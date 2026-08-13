# Use the Git CLI as the repository-semantics authority

The Workspace Inspector invokes the installed Git executable with structured arguments and consumes machine-readable output instead of implementing status and diff semantics through a Rust Git library. This preserves Git's behavior for attributes, filters, LFS, rename detection, worktrees, and repository formats; when Git is unavailable, only the Changes View is unavailable and file inspection continues to work.

## Considered Options

Pure Rust and libgit2-based implementations would remove the Git executable dependency, but could diverge from the user's installed Git behavior and make compatibility the application's responsibility.
