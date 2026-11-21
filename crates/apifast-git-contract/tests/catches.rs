//! The contract fails an adapter that does not do what it says: one that
//! finds every repository and sees no change in any.

use std::path::Path;

use apifast_app::{GitError, GitRepository, GitStatus};

struct Blind;

impl GitRepository for Blind {
    fn status(&self) -> Result<GitStatus, GitError> {
        Ok(GitStatus::default())
    }

    fn head_file(&self, _: &str) -> Result<Option<Vec<u8>>, GitError> {
        Ok(None)
    }
}

#[expect(clippy::unnecessary_wraps, reason = "the contract's `Open`")]
fn open(_: &Path) -> Option<Box<dyn GitRepository>> {
    Some(Box::new(Blind))
}

#[test]
#[should_panic(expected = "assertion")]
fn a_repository_where_there_is_none_fails() {
    apifast_git_contract::a_collection_outside_any_repository_has_none(&open);
}

#[test]
#[should_panic(expected = "assertion")]
fn an_adapter_that_sees_no_change_fails() {
    apifast_git_contract::changes_are_said_against_the_last_commit_staged_or_not(&open);
}
