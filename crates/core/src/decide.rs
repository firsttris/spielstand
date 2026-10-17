//! What to do with a game before it starts, decided by ancestry, never by clock time.
//!
//! The four cases of the safety concept, plus the two edges that keep them honest:
//! a disk that already equals the head, and a base that is an unresolved branch.

/// What this device knows about its own disk.
#[derive(Clone, Copy, Debug, Default)]
pub struct Local<'a> {
    /// The version the disk was last known to hold.
    pub base: Option<&'a str>,
    /// Content hash of `base`.
    pub base_hash: Option<&'a str>,
    /// Content hash of the save files on disk now; `None` when there are none.
    pub disk_hash: Option<&'a str>,
    /// `base` was uploaded but the server kept it as a branch (a conflict is open).
    pub base_is_open_branch: bool,
}

/// What the server says about the game.
#[derive(Clone, Copy, Debug, Default)]
pub struct Remote<'a> {
    pub head: Option<&'a str>,
    pub head_hash: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Disk, base and head agree.
    Nothing,
    /// The disk already holds the head's content; only the base has to be recorded.
    Adopt { version: String },
    /// Only the server moved on: back up the disk if needed, then write the head.
    FastForward { version: String },
    /// Only the disk changed: back it up and upload.
    Upload,
    /// Both changed since the last sync: ask, keep both.
    Conflict { head: String },
}

pub fn decide(local: Local<'_>, remote: Remote<'_>) -> Decision {
    if let (Some(disk), Some(head), Some(head_hash)) = (local.disk_hash, remote.head, remote.head_hash)
        && disk == head_hash
    {
        return if local.base == Some(head) {
            Decision::Nothing
        } else {
            Decision::Adopt {
                version: head.to_string(),
            }
        };
    }

    let disk_changed = local.disk_hash.is_some() && local.disk_hash != local.base_hash;
    let remote_changed = remote.head.is_some() && remote.head != local.base;

    match (remote.head, disk_changed, remote_changed) {
        (Some(head), _, true) if local.base_is_open_branch => Decision::Conflict {
            head: head.to_string(),
        },
        (_, false, false) => Decision::Nothing,
        (Some(head), false, true) => Decision::FastForward {
            version: head.to_string(),
        },
        (_, true, false) => Decision::Upload,
        (Some(head), true, true) => Decision::Conflict {
            head: head.to_string(),
        },
        (None, _, true) => unreachable!("remote_changed implies a head"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local<'a>(base: Option<&'a str>, base_hash: Option<&'a str>, disk: Option<&'a str>) -> Local<'a> {
        Local {
            base,
            base_hash,
            disk_hash: disk,
            base_is_open_branch: false,
        }
    }

    fn remote<'a>(head: Option<&'a str>, hash: Option<&'a str>) -> Remote<'a> {
        Remote {
            head,
            head_hash: hash,
        }
    }

    #[test]
    fn nothing_when_all_agree() {
        let d = decide(
            local(Some("v5"), Some("h5"), Some("h5")),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::Nothing);
    }

    #[test]
    fn fast_forward_when_only_the_server_moved() {
        let d = decide(
            local(Some("v4"), Some("h4"), Some("h4")),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::FastForward { version: "v5".into() });
    }

    #[test]
    fn upload_when_only_the_disk_changed() {
        let d = decide(
            local(Some("v5"), Some("h5"), Some("hX")),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::Upload);
    }

    #[test]
    fn conflict_when_both_changed() {
        let d = decide(
            local(Some("v4"), Some("h4"), Some("hX")),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::Conflict { head: "v5".into() });
    }

    #[test]
    fn first_device_uploads() {
        let d = decide(local(None, None, Some("h1")), remote(None, None));
        assert_eq!(d, Decision::Upload);
    }

    #[test]
    fn new_device_without_saves_fast_forwards() {
        let d = decide(local(None, None, None), remote(Some("v5"), Some("h5")));
        assert_eq!(d, Decision::FastForward { version: "v5".into() });
    }

    #[test]
    fn new_device_with_other_saves_conflicts() {
        let d = decide(local(None, None, Some("hX")), remote(Some("v5"), Some("h5")));
        assert_eq!(d, Decision::Conflict { head: "v5".into() });
    }

    #[test]
    fn disk_equal_to_head_is_adopted() {
        let d = decide(local(None, None, Some("h5")), remote(Some("v5"), Some("h5")));
        assert_eq!(d, Decision::Adopt { version: "v5".into() });
        let d = decide(
            local(Some("v4"), Some("h4"), Some("h5")),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::Adopt { version: "v5".into() });
    }

    #[test]
    fn open_branch_is_never_fast_forwarded() {
        let mut l = local(Some("b6"), Some("h6"), Some("h6"));
        l.base_is_open_branch = true;
        let d = decide(l, remote(Some("v6"), Some("h7")));
        assert_eq!(d, Decision::Conflict { head: "v6".into() });
    }

    #[test]
    fn missing_saves_with_known_base_wait_for_the_server() {
        let d = decide(
            local(Some("v5"), Some("h5"), None),
            remote(Some("v5"), Some("h5")),
        );
        assert_eq!(d, Decision::Nothing);
        let d = decide(
            local(Some("v5"), Some("h5"), None),
            remote(Some("v6"), Some("h6")),
        );
        assert_eq!(d, Decision::FastForward { version: "v6".into() });
    }
}
