"""Getting source code onto the volume and keeping it current.

  registry.py    the schema, plus which project/stream pairs are searchable
  projects.py    project records
  streams.py     streams and the repository sources that feed them
  schedule.py    the refresh timetable and the history of refresh runs
  migrate.py     adopting directories that arrived without configuration
  providers/     one adapter per git host
  git_ops.py     branch selection and the git invocations themselves
  ssh.py         SSH keys for cloning
  sync.py        orchestration: what to refresh, in what order
  sync_worker.py the per-repository loop
  notify.py      telling the indexer there is new source to read

The dependency direction is one-way: sync depends on sync_worker, which depends
on git_ops and providers, which depend on nothing else here. Nothing lower ever
imports something higher.
"""
from .git_ops import (
    clone_repository,
    current_branch,
    pick_branch,
    pull_repository,
    remove_local_checkout,
    sync_local_checkout,
    write_sync_report,
)
from .migrate import import_filesystem_projects
from .notify import notify_indexer
from .projects import (
    add_project,
    edit_project,
    fetch_project,
    list_projects_with_detail,
    remove_project,
)
from .providers import RemoteRepo, RepositoryProvider, resolve_provider
from .registry import (
    build_workspace_registry,
    ensure_registry_schema,
    registry_key_for,
)
from .schedule import (
    fetch_refresh_job,
    get_refresh_schedule,
    list_refresh_job_history,
    record_refresh_progress,
    record_stream_refresh_result,
    refresh_in_progress,
    set_refresh_schedule,
    start_refresh_job,
)
from .ssh import (
    git_ssh_command_for_key,
    ssh_private_key_tempfile,
    verify_ssh_private_key,
)
from .streams import (
    attach_repository_source,
    add_stream,
    detach_repository_source,
    edit_repository_source,
    fetch_stream,
    list_active_streams,
    list_repository_sources,
    remove_stream,
    set_stream_enabled,
)
from .sync import sync_all_projects, sync_stream

__all__ = [
    "RemoteRepo",
    "RepositoryProvider",
    "add_project",
    "add_stream",
    "attach_repository_source",
    "build_workspace_registry",
    "clone_repository",
    "current_branch",
    "detach_repository_source",
    "edit_project",
    "edit_repository_source",
    "ensure_registry_schema",
    "fetch_project",
    "fetch_refresh_job",
    "fetch_stream",
    "get_refresh_schedule",
    "git_ssh_command_for_key",
    "import_filesystem_projects",
    "list_active_streams",
    "list_projects_with_detail",
    "list_refresh_job_history",
    "list_repository_sources",
    "notify_indexer",
    "pick_branch",
    "pull_repository",
    "clone_repository",
    "record_refresh_progress",
    "record_stream_refresh_result",
    "refresh_in_progress",
    "registry_key_for",
    "remove_local_checkout",
    "remove_project",
    "remove_stream",
    "resolve_provider",
    "set_refresh_schedule",
    "set_stream_enabled",
    "ssh_private_key_tempfile",
    "start_refresh_job",
    "sync_all_projects",
    "sync_local_checkout",
    "sync_stream",
    "verify_ssh_private_key",
    "write_sync_report",
]
