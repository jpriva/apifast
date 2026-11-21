# apifast-git

**Layer:** Adapter

The git repository a collection lives in, read and written with [gitoxide](https://github.com/GitoxideLabs/gitoxide) (`gix`): the branch, what changed since the last commit, and files as they were in it. No `git` binary is needed. A collection may be a folder of a larger repository: only its files are seen, by paths relative to it.

**Allowed workspace dependencies:** `apifast-domain`, `apifast-app`. See the [architecture](../../docs/03-arquitectura.md) (in Spanish) and [ADR 0014](../../docs/adr/0014-git-con-gitoxide.md).
