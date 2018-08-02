import subprocess

def return_with_pull_metadata(get_pulls):
    pulls = get_pulls()
    pulls_update_mergeable = lambda: [p for p in pulls if p.mergeable is None and not p.merged]
    print('Fetching open pulls metadata ...')
    while pulls_update_mergeable():
        print('Update mergable state for pulls {}'.format([p.number for p in pulls_update_mergeable()]))
        [p.update() for p in pulls_update_mergeable()]
        pulls = get_pulls()
    return pulls


def call_git(args, **kwargs):
    subprocess.check_call(['git'] + args, **kwargs)


def get_git(args):
    return subprocess.check_output(['git'] + args, universal_newlines=True).strip()