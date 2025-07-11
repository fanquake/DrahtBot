use clap::Parser;
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

#[derive(Clone)]
struct SlugTok {
    owner: String,
    repo: String,
    ci_token: String,
}

impl std::str::FromStr for SlugTok {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Format: a/b:c
        let err = "Wrong format, see --help.";
        let mut it = s.split(':');
        let mut it_slug = it.next().ok_or(err)?.split('/');
        let res = Self {
            owner: it_slug.next().ok_or(err)?.to_string(),
            repo: it_slug.next().ok_or(err)?.to_string(),
            ci_token: it.next().ok_or(err)?.to_string(),
        };
        if it.next().is_none() && it_slug.next().is_none() {
            return Ok(res);
        }
        Err(err)
    }
}

#[derive(clap::Parser)]
#[command(about = "Trigger Cirrus CI to re-run.", long_about = None)]
struct Args {
    /// The access token for GitHub.
    #[arg(long)]
    github_access_token: Option<String>,
    /// The repo slugs of the remotes on GitHub. Format: owner/repo:cirrus_org_token
    #[arg(long)]
    github_repo: Vec<SlugTok>,
    /// The task names to re-run.
    #[arg(long)]
    task: Vec<String>,
    /// How many minutes to sleep between pulls.
    #[arg(long, default_value_t = 25)]
    sleep_min: u64,
    /// Print changes/edits instead of calling the GitHub/CI API.
    #[arg(long, default_value_t = false)]
    dry_run: bool,
}

static ERROR_JSON_FORMAT: &str = "json format error";

fn rerun_first(
    task_name: &str,
    tasks: &[serde_json::Value],
    token: &String,
    dry_run: bool,
) -> Result<(), String> {
    let mut task = None;
    for t in tasks {
        let name = t["name"].as_str().ok_or(format!(
            "{ERROR_JSON_FORMAT}: Missing '{key}' in '{t}'",
            key = "name",
        ))?;
        if name.contains(task_name) {
            task = Some(t);
            break;
        }
    }
    if task.is_none() {
        return Ok(());
    }
    let task = task.unwrap();
    let t_id = task["id"].as_str().ok_or(format!(
        "{ERROR_JSON_FORMAT}: Missing {key} in '{task}'",
        key = "id",
    ))?;
    let t_name = task["name"].as_str().ok_or(format!(
        "{ERROR_JSON_FORMAT}: Missing {key} in '{task}'",
        key = "name",
    ))?;
    let raw_data = format!(
        r#"
                        {{
                            "query":"mutation
                            {{
                               rerun(
                                 input: {{
                                   attachTerminal: false, clientMutationId: \"rerun-{t_id}\", taskId: \"{t_id}\"
                                 }}
                               ) {{
                                  newTask {{
                                    id
                                  }}
                               }}
                             }}"
                         }}
                     "#
    );
    println!("Re-run task {t_name} (id: {t_id})");
    if !dry_run {
        let out = util::check_output(std::process::Command::new("curl").args([
            "https://api.cirrus-ci.com/graphql",
            "-X",
            "POST",
            "-H",
            &format!("Authorization: Bearer {token}"),
            "--data-raw",
            &raw_data,
        ]));
        println!("{out}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> octocrab::Result<()> {
    let args = Args::parse();

    let github = util::get_octocrab(args.github_access_token)?;

    for SlugTok {
        owner,
        repo,
        ci_token,
    } in args.github_repo
    {
        println!("Get open pulls for {}/{} ...", owner, repo);
        let pulls_api = github.pulls(&owner, &repo);
        let pulls = {
            let mut pulls = github
                .all_pages(
                    pulls_api
                        .list()
                        .state(octocrab::params::State::Open)
                        .send()
                        .await?,
                )
                .await?;
            // Rotate the vector to start at a different place each time, to account for
            // api.cirrus-ci network errors, which would abort the program. On the next start, it
            // would start iterating from the same place.
            let rotate = RandomState::new().build_hasher().finish() as usize % (pulls.len());
            pulls.rotate_left(rotate);
            pulls
        };
        println!("Open pulls: {}", pulls.len());
        for (i, pull) in pulls.iter().enumerate() {
            println!(
                "{}/{} (Pull: {}/{}#{})",
                i,
                pulls.len(),
                owner,
                repo,
                pull.number
            );
            let pull = util::get_pull_mergeable(&pulls_api, pull.number).await?;
            let pull = match pull {
                None => {
                    continue;
                }
                Some(p) => p,
            };
            if !pull.mergeable.unwrap() {
                continue;
            }
            let pull_num = pull.number;
            let raw_data = format!(
                r#"
                    {{
                        "query":"query
                        {{
                            ownerRepository(platform: \"github\", owner: \"{owner}\", name: \"{repo}\") {{
                              viewerPermission
                              builds(last: 1, branch: \"pull/{pull_num}\") {{
                                edges {{
                                  node {{
                                    tasks {{
                                      id
                                      name
                                    }}
                                  }}
                                }}
                              }}
                            }}
                        }}"
                     }}
                "#
            );
            let output = util::check_output(std::process::Command::new("curl").args([
                "https://api.cirrus-ci.com/graphql",
                "-X",
                "POST",
                "--data-raw",
                &raw_data,
            ]));
            let tasks = serde_json::from_str::<serde_json::value::Value>(&output)
                .map_err(|e| e.to_string())
                .and_then(|json_parsed| {
                    json_parsed["data"]["ownerRepository"]["builds"]["edges"][0]["node"]["tasks"]
                        .as_array()
                        .cloned()
                        .ok_or(format!("{ERROR_JSON_FORMAT}: Missing keys in '{output}'"))
                });
            if let Err(msg) = tasks {
                println!("{msg}");
                continue;
            }
            let tasks = tasks.unwrap();
            for task_name in &args.task {
                if let Err(msg) = rerun_first(task_name, &tasks, &ci_token, args.dry_run) {
                    println!("{msg}");
                }
            }
            std::thread::sleep(std::time::Duration::from_secs(args.sleep_min * 60));
        }
    }
    Ok(())
}
