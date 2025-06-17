mod config;
mod errors;
mod features;

use std::str::FromStr;

use crate::features::spam_detection::SpamDetectionFeature;
use crate::features::summary_comment::SummaryCommentFeature;
use actix_web::{get, post, web, App, HttpRequest, HttpServer, Responder};
use clap::Parser;
use features::Feature;
use lazy_static::lazy_static;
use octocrab::Octocrab;
use strum::{Display, EnumString};

use crate::config::Config;
use crate::errors::{DrahtBotError, Result};

#[derive(Parser)]
#[command(about="Run features on webhooks", long_about = None)]
struct Args {
    #[arg(long, help = "GitHub token")]
    token: String,
    #[arg(long, help = "LLM token", default_value = "")]
    llm_token: String,
    #[arg(long, help = "Host to listen on", default_value = "localhost")]
    host: String,
    #[arg(long, help = "Port to listen on", default_value = "1337")]
    port: u16,
    /// The path to the yaml config file.
    #[arg(long)]
    config_file: std::path::PathBuf,
    /// Print changes/edits instead of calling the GitHub/CI API.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

#[derive(Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "snake_case")]
pub enum GitHubEvent {
    CheckSuite,
    IssueComment,
    PullRequest,
    PullRequestReview,

    Unknown,
}

#[get("/")]
async fn index() -> &'static str {
    "Welcome to DrahtBot!"
}

pub struct Context {
    octocrab: Octocrab,
    bot_username: String,
    pub config: Config,
    llm_token: String,
    dry_run: bool,
}

#[post("/drahtbot")]
async fn postreceive_handler(
    ctx: web::Data<Context>,
    req: HttpRequest,
    data: web::Json<serde_json::Value>,
) -> impl Responder {
    let event_str = req
        .headers()
        .get("X-GitHub-Event")
        .unwrap()
        .to_str()
        .unwrap();
    let event = GitHubEvent::from_str(event_str).unwrap_or(GitHubEvent::Unknown);

    emit_event(&ctx, event, data).await.unwrap();

    "OK"
}

fn features() -> Vec<Box<dyn Feature>> {
    vec![
        Box::new(SummaryCommentFeature::new()),
        Box::new(crate::features::ci_status::CiStatusFeature::new()),
        Box::new(crate::features::labels::LabelsFeature::new()),
        Box::new(SpamDetectionFeature::new()),
    ]
}

lazy_static! {
    static ref MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::new(());
}

async fn emit_event(
    ctx: &Context,
    event: GitHubEvent,
    data: web::Json<serde_json::Value>,
) -> Result<()> {
    let _guard = MUTEX.lock().await;

    for feature in features() {
        if feature.meta().events().contains(&event) {
            feature.handle(ctx, &event, &data).await?;
        }
    }

    Ok(())
}

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let config: Config = serde_yaml::from_reader(
        std::fs::File::open(args.config_file).expect("config file path error"),
    )
    .expect("yaml error");

    let octocrab = octocrab::Octocrab::builder()
        .personal_token(args.token)
        .build()
        .map_err(DrahtBotError::GitHubError)?;

    println!("DrahtBot will will run the following features:");
    for feature in features() {
        println!(" - {}", feature.meta().name());
        println!("   {}", feature.meta().description());
    }

    println!();

    // Get the bot's username
    let bot_username = octocrab
        .current()
        .user()
        .await
        .map_err(DrahtBotError::GitHubError)?
        .login;

    println!("Running as {bot_username}...");

    let context = web::Data::new(Context {
        octocrab,
        bot_username,
        config,
        llm_token: args.llm_token,
        dry_run: args.dry_run,
    });

    HttpServer::new(move || {
        App::new()
            .app_data(context.clone())
            .service(index)
            .service(postreceive_handler)
    })
    .bind(format!("{}:{}", args.host, args.port))?
    .run()
    .await?;
    Ok(())
}
