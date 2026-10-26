mod support;

use flate2::write::GzEncoder;
use flate2::Compression;
use arca_core::skills::{
    resolve_source, GithubEndpoints, SkillError, SkillSource, SkillSourceInput, SkillsPaths,
    MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, MAX_DOWNLOAD_BYTES, MAX_SINGLE_FILE_BYTES,
};
use arca_core::testenv::TestHome;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use support::skills::{write_skill, MockGithub, FIXTURE_SHA};
use tar::{Builder, EntryType, Header};
use url::Url;
use uuid::Uuid;

#[test]
fn local_folder_is_copied_not_linked_and_can_contain_multiple_skills() {
    let th = TestHome::new("skills-local-source");
    let source = th.home.join("source");
    write_skill(&source.join("alpha"), "alpha", "Alpha fixture");
    write_skill(&source.join("beta"), "beta", "Beta fixture");
    let result = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    )
    .unwrap();
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta"]
    );
    fs::write(source.join("alpha/SKILL.md"), "changed after resolve").unwrap();
    assert_ne!(
        fs::read_to_string(th.home.join(format!(
            ".arca/staging/skills/{}/candidates/alpha/SKILL.md",
            result.operation_id,
        )))
        .unwrap(),
        "changed after resolve"
    );
}

#[cfg(unix)]
#[test]
fn source_resolution_rejects_a_symlinked_staging_parent_without_writing_outside_arca() {
    let th = TestHome::new("skills-source-staging-parent");
    let source = th.home.join("source");
    write_skill(&source.join("safe"), "safe", "Safe fixture");
    let paths = SkillsPaths::from_env().unwrap();
    let staging_parent = paths.arca_dir().join("staging");
    let retained_parent = paths.arca_dir().join("retained-staging");
    fs::rename(&staging_parent, &retained_parent).unwrap();
    let outside = th.home.join("outside-staging");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("marker"), "keep").unwrap();
    symlink(&outside, &staging_parent).unwrap();

    let result = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    );
    assert!(matches!(
        result,
        Err(SkillError::UnsafePath { .. }) | Err(SkillError::InvalidSource { .. })
    ));
    assert_eq!(fs::read_to_string(outside.join("marker")).unwrap(), "keep");
    assert!(!outside.join("skills").exists());
}

#[cfg(unix)]
#[test]
fn source_pipeline_stays_on_operation_fd_after_staging_parent_swap() {
    let th = TestHome::new("skills-source-operation-fd");
    let (server, reached, resume) = ScriptedGithub::paused_archive(valid_archive(&["fd-safe"]));
    let endpoints = server.endpoints();
    let resolver = thread::spawn(move || {
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            endpoints,
        )
    });
    reached
        .recv_timeout(Duration::from_secs(5))
        .expect("resolver did not pause at the archive request");
    let (retained_operation, outside_operation) = substitute_staging_parent(&th);
    resume.send(()).unwrap();

    let resolution = resolver.join().unwrap().unwrap();

    assert_eq!(directory_names(&outside_operation), vec!["sentinel"]);
    assert!(retained_operation
        .join("candidates")
        .join("fd-safe")
        .join("SKILL.md")
        .exists());
    assert_eq!(
        resolution
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        vec!["fd-safe"]
    );
}

#[cfg(unix)]
#[test]
fn source_failure_cleanup_removes_only_anchored_operation_after_parent_swap() {
    let th = TestHome::new("skills-source-operation-cleanup-fd");
    let (server, reached, resume) = ScriptedGithub::paused_archive(b"not-a-gzip".to_vec());
    let endpoints = server.endpoints();
    let resolver = thread::spawn(move || {
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            endpoints,
        )
    });
    reached
        .recv_timeout(Duration::from_secs(5))
        .expect("resolver did not pause at the archive request");
    let (retained_operation, outside_operation) = substitute_staging_parent(&th);
    resume.send(()).unwrap();

    assert!(resolver.join().unwrap().is_err());
    assert_eq!(directory_names(&outside_operation), vec!["sentinel"]);
    assert!(!retained_operation.exists());
}

#[cfg(unix)]
fn substitute_staging_parent(th: &TestHome) -> (std::path::PathBuf, std::path::PathBuf) {
    let paths = SkillsPaths::from_env().unwrap();
    let operation_ids = fs::read_dir(paths.staging_skills_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(operation_ids.len(), 1);
    let operation_id = &operation_ids[0];
    let staging_parent = paths.arca_dir().join("staging");
    let retained_parent = th.home.join("retained-staging-parent");
    fs::rename(&staging_parent, &retained_parent).unwrap();

    let outside = th.home.join("outside-staging-parent");
    let outside_operation = outside.join("skills").join(operation_id);
    fs::create_dir_all(&outside_operation).unwrap();
    fs::write(outside_operation.join("sentinel"), "keep").unwrap();
    symlink(&outside, &staging_parent).unwrap();

    (
        retained_parent.join("skills").join(operation_id),
        outside_operation,
    )
}

fn directory_names(path: &Path) -> Vec<String> {
    let mut names = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn github_tree_url_resolves_ref_to_sha_without_git() {
    let th = TestHome::new("skills-github-source");
    let server = MockGithub::start(&["review", "release"]);
    let result = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills/tree/main/catalog".into(),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_eq!(result.resolved_revision.as_deref(), Some(FIXTURE_SHA));
    assert!(matches!(
        result.source,
        SkillSource::Github { ref subpath, .. } if subpath == "catalog"
    ));
    assert_eq!(
        server.requests(),
        vec!["commit:main/catalog", "commit:main", "archive"]
    );
    drop(th);
}

#[test]
fn rejects_private_auth_redirects_and_oversized_archives() {
    let _th = TestHome::new("skills-source-rejections");
    for value in [
        "git@github.com:acme/private.git",
        "https://user:token@github.com/acme/private",
    ] {
        assert!(matches!(
            resolve_source(
                SkillSourceInput::Github {
                    value: value.into()
                },
                GithubEndpoints::production(),
            ),
            Err(SkillError::InvalidSource { .. })
        ));
    }

    let redirect = MockGithub::redirect_to("https://example.com/archive.tar.gz");
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            redirect.endpoints(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));

    let oversized = MockGithub::oversized_download(MAX_DOWNLOAD_BYTES + 1);
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            oversized.endpoints(),
        ),
        Err(SkillError::LimitExceeded { limit, .. }) if limit == "download"
    ));
}

#[test]
fn repository_input_uses_default_branch_and_returns_validated_summaries() {
    let th = TestHome::new("skills-repository-source");
    let server = MockGithub::start(&["zulu", "alpha"]);
    let result = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills".into(),
        },
        server.endpoints(),
    )
    .unwrap();

    assert_eq!(server.requests(), vec!["repo", "commit:main", "archive"]);
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zulu"]
    );
    assert!(result.candidates.iter().all(|candidate| {
        candidate.content_hash.len() == 64 && candidate.file_count == 1 && candidate.total_bytes > 0
    }));
    assert!(matches!(
        result.source,
        SkillSource::Github {
            ref owner,
            ref repo,
            ref requested_ref,
            ref subpath,
            pinned: false,
        } if owner == "acme" && repo == "skills" && requested_ref == "main" && subpath.is_empty()
    ));
    assert_eq!(
        Uuid::parse_str(&result.operation_id)
            .unwrap()
            .hyphenated()
            .to_string(),
        result.operation_id
    );
    let wire = serde_json::to_string(&result).unwrap();
    assert!(!wire.contains("/.arca/staging/"));
    assert!(!wire.contains(th.home.to_string_lossy().as_ref()));
    assert_private_directory(
        &th.home
            .join(format!(".arca/staging/skills/{}", result.operation_id)),
    );
}

#[test]
fn local_metadata_is_home_collapsed_and_candidates_are_sorted_by_name_then_path() {
    let th = TestHome::new("skills-local-metadata");
    let source = th.home.join("picked");
    write_skill(&source.join("nested/alpha"), "alpha", "Alpha fixture");
    write_skill(&source.join("zeta"), "zeta", "Zeta fixture");
    let result = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    )
    .unwrap();
    assert!(matches!(
        result.source,
        SkillSource::Local { ref path, ref subpath }
            if path == "~/picked" && subpath.is_empty()
    ));
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|row| (row.name.as_str(), row.relative_path.as_str()))
            .collect::<Vec<_>>(),
        vec![("alpha", "nested/alpha"), ("zeta", "zeta")]
    );
}

#[test]
fn rejects_duplicate_manifest_names_and_nested_candidate_trees() {
    let th = TestHome::new("skills-local-overlap");
    let duplicate = th.home.join("duplicate");
    write_skill(&duplicate.join("one/shared"), "shared", "First");
    write_skill(&duplicate.join("two/shared"), "shared", "Second");
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Local {
                path: duplicate.display().to_string()
            },
            GithubEndpoints::production(),
        ),
        Err(SkillError::Conflict { .. })
    ));

    let nested = th.home.join("nested");
    write_skill(&nested.join("outer"), "outer", "Outer");
    write_skill(&nested.join("outer/inner"), "inner", "Inner");
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Local {
                path: nested.display().to_string()
            },
            GithubEndpoints::production(),
        ),
        Err(SkillError::Conflict { .. })
    ));
}

#[test]
fn selected_skill_root_is_one_candidate_and_its_snapshot_is_independent() {
    let th = TestHome::new("skills-local-root");
    let source = th.home.join("direct");
    write_skill(&source, "direct", "Direct fixture");
    write_skill(&source.join("nested"), "nested", "Nested content");
    let result = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    )
    .unwrap();

    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].name, "direct");
    assert_eq!(result.candidates[0].relative_path, "");
    fs::remove_dir_all(&source).unwrap();
    assert!(th
        .home
        .join(format!(
            ".arca/staging/skills/{}/candidates/direct/nested/SKILL.md",
            result.operation_id
        ))
        .is_file());
}

#[test]
fn invalid_sources_are_bounded_canonical_and_never_probe_the_network() {
    let _th = TestHome::new("skills-invalid-sources");
    let too_many = (0..17)
        .map(|index| format!("p{index}"))
        .collect::<Vec<_>>()
        .join("/");
    for value in [
        "ssh://git@github.com/acme/skills",
        "github.com:acme/skills",
        "https://github.com//skills",
        "https://github.com/acme/",
        "https://gitlab.com/acme/skills",
        "https://github.com/acme/skills.git",
        "https://github.com/acme/skills?token=secret",
        "https://github.com/acme/skills#fragment",
        "https://github.com/acme%2Fother/skills",
        "https://github.com/acme/skills/tree/main/%2e%2e/secret",
        "https://github.com/acme/skills/tree/main/bad%00path",
    ] {
        assert!(
            matches!(
                resolve_source(
                    SkillSourceInput::Github {
                        value: value.into()
                    },
                    GithubEndpoints::production(),
                ),
                Err(SkillError::InvalidSource { .. })
            ),
            "accepted unsafe source {value}"
        );
    }
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: format!("https://github.com/acme/skills/tree/{too_many}")
            },
            GithubEndpoints::production(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "a".repeat(4097)
            },
            GithubEndpoints::production(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn exact_sha_tree_refs_are_marked_pinned() {
    let _th = TestHome::new("skills-pinned-source");
    let server = MockGithub::start(&["review"]);
    let result = resolve_source(
        SkillSourceInput::Github {
            value: format!("https://github.com/acme/skills/tree/{FIXTURE_SHA}/catalog"),
        },
        server.endpoints(),
    )
    .unwrap();
    assert!(matches!(
        result.source,
        SkillSource::Github { pinned: true, ref requested_ref, .. } if requested_ref == FIXTURE_SHA
    ));
    assert_eq!(
        server.requests(),
        vec![format!("commit:{FIXTURE_SHA}"), "archive".into()]
    );
}

#[test]
fn pinned_sha_can_select_a_deep_subtree_without_consuming_ref_probes() {
    let _th = TestHome::new("skills-pinned-deep");
    let subpath = (0..20)
        .map(|index| format!("level-{index}"))
        .collect::<Vec<_>>()
        .join("/");
    let archive = archive_with(&[ArchiveEntry::File(
        format!("skills-{FIXTURE_SHA}/{subpath}/review/SKILL.md"),
        valid_manifest("review"),
    )]);
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
    let result = resolve_source(
        SkillSourceInput::Github {
            value: format!("https://github.com/acme/skills/tree/{FIXTURE_SHA}/{subpath}"),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_eq!(result.candidates[0].name, "review");
    assert!(matches!(
        result.source,
        SkillSource::Github { pinned: true, .. }
    ));
}

#[test]
fn relative_redirects_work_but_six_redirects_are_rejected() {
    let _th = TestHome::new("skills-redirect-source");
    let relative = MockGithub::redirect_to("/relative-archive");
    let resolved = resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into(),
        },
        relative.endpoints(),
    )
    .unwrap();
    assert_eq!(resolved.candidates[0].name, "review");
    assert_eq!(
        relative.requests(),
        vec!["repo", "commit:main", "archive", "archive"]
    );

    let five = ScriptedGithub::new(ArchiveBehavior::Redirects {
        count: 5,
        archive: valid_archive(&["review"]),
    });
    assert!(resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into(),
        },
        five.endpoints(),
    )
    .is_ok());

    let chain = ScriptedGithub::new(ArchiveBehavior::Redirects {
        count: 6,
        archive: valid_archive(&["review"]),
    });
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            chain.endpoints(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn streamed_download_size_is_enforced_without_trusting_content_length() {
    let _th = TestHome::new("skills-stream-limit");
    let server = ScriptedGithub::new(ArchiveBehavior::Chunked(MAX_DOWNLOAD_BYTES + 1));
    let result = resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into(),
        },
        server.endpoints(),
    );
    assert!(
        matches!(
            result,
            Err(SkillError::LimitExceeded { ref limit, .. }) if limit == "download"
        ),
        "unexpected streamed-limit result: {result:?}"
    );
}

#[test]
fn archive_paths_hardlinks_specials_and_duplicates_are_rejected() {
    let cases = [
        archive_with(&[ArchiveEntry::RawFile(
            format!("skills-{FIXTURE_SHA}/../escape/SKILL.md"),
            valid_manifest("escape"),
        )]),
        archive_with(&[ArchiveEntry::RawFile(
            "/tmp/arca-escape/SKILL.md".into(),
            valid_manifest("arca-escape"),
        )]),
        archive_with(&[ArchiveEntry::Link {
            path: format!("skills-{FIXTURE_SHA}/catalog/review/hard"),
            target: format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
            kind: EntryType::hard_link(),
        }]),
        archive_with(&[ArchiveEntry::Link {
            path: format!("skills-{FIXTURE_SHA}/catalog/review/fifo"),
            target: String::new(),
            kind: EntryType::fifo(),
        }]),
        archive_with(&[
            ArchiveEntry::File(
                format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
                valid_manifest("review"),
            ),
            ArchiveEntry::File(
                format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
                valid_manifest("review"),
            ),
        ]),
    ];
    for (index, archive) in cases.into_iter().enumerate() {
        let _th = TestHome::new(&format!("skills-tar-{index}"));
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
        let error = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                SkillError::InvalidSource { .. }
                    | SkillError::UnsafePath { .. }
                    | SkillError::Conflict { .. }
            ),
            "unexpected archive error: {error:?}"
        );
    }
}

#[test]
fn archive_symlinks_must_resolve_inside_the_extracted_tree() {
    for (index, target) in [
        "../../../../outside",
        "missing",
        "/absolute/target",
        "bad\\target",
    ]
    .into_iter()
    .enumerate()
    {
        let _th = TestHome::new(&format!("skills-link-{index}"));
        let archive = archive_with(&[
            ArchiveEntry::File(
                format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
                valid_manifest("review"),
            ),
            ArchiveEntry::Link {
                path: format!("skills-{FIXTURE_SHA}/catalog/review/guide"),
                target: target.into(),
                kind: EntryType::symlink(),
            },
        ]);
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
        let result = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        );
        assert!(
            matches!(
                result,
                Err(SkillError::InvalidSource { .. }) | Err(SkillError::UnsafePath { .. })
            ),
            "unexpected symlink result for {target}: {result:?}"
        );
    }

    let _th = TestHome::new("skills-link-valid");
    let archive = archive_with(&[
        ArchiveEntry::File(
            format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
            valid_manifest("review"),
        ),
        ArchiveEntry::File(
            format!("skills-{FIXTURE_SHA}/catalog/review/guide.md"),
            b"guide".to_vec(),
        ),
        ArchiveEntry::Link {
            path: format!("skills-{FIXTURE_SHA}/catalog/review/first"),
            target: "second".into(),
            kind: EntryType::symlink(),
        },
        ArchiveEntry::Link {
            path: format!("skills-{FIXTURE_SHA}/catalog/review/second"),
            target: "guide.md".into(),
            kind: EntryType::symlink(),
        },
    ]);
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
    assert!(resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into()
        },
        server.endpoints(),
    )
    .is_ok());
}

#[test]
fn archive_symlink_graph_rejects_cycles_and_parent_link_collisions() {
    let root = format!("skills-{FIXTURE_SHA}/catalog/review");
    let cases = [
        archive_with(&[
            ArchiveEntry::File(format!("{root}/SKILL.md"), valid_manifest("review")),
            ArchiveEntry::Link {
                path: format!("{root}/a"),
                target: "b".into(),
                kind: EntryType::symlink(),
            },
            ArchiveEntry::Link {
                path: format!("{root}/b"),
                target: "a".into(),
                kind: EntryType::symlink(),
            },
        ]),
        archive_with(&[
            ArchiveEntry::File(format!("{root}/SKILL.md"), valid_manifest("review")),
            ArchiveEntry::Link {
                path: format!("{root}/parent"),
                target: ".".into(),
                kind: EntryType::symlink(),
            },
            ArchiveEntry::File(format!("{root}/parent/child"), b"child".to_vec()),
        ]),
    ];

    for (index, archive) in cases.into_iter().enumerate() {
        let home = TestHome::new(&format!("skills-link-graph-{index}"));
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
        let error = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        )
        .unwrap_err();
        assert!(
            matches!(
                error,
                SkillError::InvalidSource { .. } | SkillError::UnsafePath { .. }
            ),
            "unexpected link graph error: {error:?}"
        );
        drop(server);
        drop(home);
    }

    let _nul_home = TestHome::new("skills-link-target-nul");
    let mut link = raw_header(
        &format!("{root}/link"),
        EntryType::symlink(),
        0,
        Some("good"),
    );
    link.as_mut_bytes()[157..257].fill(0);
    link.as_mut_bytes()[157..166].copy_from_slice(b"good\0tail");
    link.set_cksum();
    let archive = gzip_tar_records(
        vec![
            regular_record(&format!("{root}/SKILL.md"), valid_manifest("review")),
            regular_record(&format!("{root}/good"), b"good".to_vec()),
            (link, Vec::new()),
        ],
        2,
        &[],
    );
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn archive_symlink_graph_has_a_bounded_hop_limit() {
    let _home = TestHome::new("skills-link-hop-limit");
    let root = format!("skills-{FIXTURE_SHA}/catalog/review");
    let mut entries = vec![
        ArchiveEntry::File(format!("{root}/SKILL.md"), valid_manifest("review")),
        ArchiveEntry::File(format!("{root}/guide.md"), b"guide".to_vec()),
    ];
    for index in 0..=64 {
        entries.push(ArchiveEntry::Link {
            path: format!("{root}/link-{index}"),
            target: if index == 64 {
                "guide.md".into()
            } else {
                format!("link-{}", index + 1)
            },
            kind: EntryType::symlink(),
        });
    }
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive_with(&entries)));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        ),
        Err(SkillError::InvalidSource { .. })
    ));
}

#[test]
fn declared_file_sizes_and_archive_entry_counts_are_bounded() {
    let th = TestHome::new("skills-tar-declared");
    let oversized = gzip_raw_tar(&[raw_header(
        &format!("skills-{FIXTURE_SHA}/catalog/review/large.bin"),
        EntryType::file(),
        MAX_SINGLE_FILE_BYTES + 1,
        None,
    )]);
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(oversized));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            server.endpoints(),
        ),
        Err(SkillError::LimitExceeded { limit, .. }) if limit == "single_file"
    ));

    drop(th);
    let _th = TestHome::new("skills-tar-entries");
    let mut entries = Vec::with_capacity(MAX_ARCHIVE_ENTRIES as usize + 1);
    for index in 0..=MAX_ARCHIVE_ENTRIES {
        entries.push(ArchiveEntry::Directory(format!(
            "skills-{FIXTURE_SHA}/empty/{index}"
        )));
    }
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive_with(&entries)));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            server.endpoints(),
        ),
        Err(SkillError::LimitExceeded { limit, .. }) if limit == "entries"
    ));
}

#[test]
fn decompressed_archive_stream_is_bounded_even_for_hidden_extension_entries() {
    let _th = TestHome::new("skills-tar-expansion");
    let archive = gzip_large_extension(MAX_ARCHIVE_BYTES + 1);
    assert!(archive.len() as u64 <= MAX_DOWNLOAD_BYTES);
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
    assert!(matches!(
        resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into()
            },
            server.endpoints(),
        ),
        Err(SkillError::LimitExceeded { limit, .. }) if limit == "archive"
    ));
}

#[test]
fn physical_tar_headers_are_counted_before_hidden_extensions() {
    let exact_home = TestHome::new("skills-tar-physical-exact");
    let exact = gzip_tar_records(physical_limit_records(), 2, &[]);
    let exact_server = ScriptedGithub::new(ArchiveBehavior::Bytes(exact));
    let exact_result = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills/tree/main/catalog".into(),
        },
        exact_server.endpoints(),
    );
    assert!(
        exact_result.is_ok(),
        "exact physical limit failed: {exact_result:?}"
    );
    drop(exact_server);
    drop(exact_home);

    let _overflow_home = TestHome::new("skills-tar-physical-overflow");
    let mut overflow = physical_limit_records();
    overflow.pop();
    overflow.push(extension_record(b'x', pax_record("comment", "hidden")));
    overflow.push(regular_record(
        &format!("skills-{FIXTURE_SHA}/outside/ignored.txt"),
        b"ignored".to_vec(),
    ));
    let overflow_server =
        ScriptedGithub::new(ArchiveBehavior::Bytes(gzip_tar_records(overflow, 2, &[])));
    let error = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills/tree/main/catalog".into(),
        },
        overflow_server.endpoints(),
    )
    .unwrap_err();
    assert!(
        matches!(error, SkillError::LimitExceeded { ref limit, actual, allowed }
            if limit == "entries"
                && actual == MAX_ARCHIVE_ENTRIES + 1
                && allowed == MAX_ARCHIVE_ENTRIES),
        "hidden physical header did not consume the entry budget: {error:?}"
    );
}

#[test]
fn ambiguous_tar_extensions_and_sparse_metadata_are_rejected() {
    let candidate_path = format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md");
    let guide_path = format!("skills-{FIXTURE_SHA}/catalog/review/guide");
    let guide_file = format!("skills-{FIXTURE_SHA}/catalog/review/guide.md");

    let mut sparse_pax = pax_record("path", &candidate_path);
    sparse_pax.extend(pax_record("GNU.sparse.map", "0,4"));
    let cases = vec![
        (
            "pax-local-sparse",
            vec![
                extension_record(b'x', sparse_pax),
                regular_record("placeholder", valid_manifest("review")),
            ],
        ),
        (
            "pax-global",
            vec![
                extension_record(b'g', pax_record("comment", "global")),
                regular_record(&candidate_path, valid_manifest("review")),
            ],
        ),
        (
            "gnu-long-name",
            vec![
                extension_record(b'L', format!("{candidate_path}\0").into_bytes()),
                regular_record("placeholder", valid_manifest("review")),
            ],
        ),
        (
            "gnu-long-link",
            vec![
                regular_record(&candidate_path, valid_manifest("review")),
                regular_record(&guide_file, b"guide".to_vec()),
                extension_record(b'K', b"guide.md\0".to_vec()),
                (
                    raw_header(&guide_path, EntryType::symlink(), 0, Some("placeholder")),
                    Vec::new(),
                ),
            ],
        ),
        (
            "gnu-sparse",
            vec![(
                raw_header(
                    &candidate_path,
                    EntryType::new(b'S'),
                    valid_manifest("review").len() as u64,
                    None,
                ),
                valid_manifest("review"),
            )],
        ),
    ];

    for (index, (name, records)) in cases.into_iter().enumerate() {
        let home = TestHome::new(&format!("skills-tar-extension-{index}"));
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(gzip_tar_records(records, 2, &[])));
        let error = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        )
        .unwrap_err();
        assert!(
            matches!(error, SkillError::InvalidSource { ref message }
                if message.contains("unsupported archive extension")),
            "{name} was not rejected by the raw extension policy: {error:?}"
        );
        drop(server);
        drop(home);
    }
}

#[test]
fn raw_ustar_path_fields_reject_hidden_tails_and_unsupported_gnu_layouts() {
    let candidate_path = format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md");
    let outside_prefix = format!("skills-{FIXTURE_SHA}/outside");
    let hidden_name = b"safe\0hidden";
    let mut hidden_prefix = outside_prefix.as_bytes().to_vec();
    hidden_prefix.extend_from_slice(b"\0hidden");
    let cases = vec![
        (
            "name-hidden-tail",
            raw_ustar_header(
                hidden_name,
                outside_prefix.as_bytes(),
                EntryType::file(),
                0,
                None,
            ),
            "name",
        ),
        (
            "prefix-hidden-tail",
            raw_ustar_header(b"ignored.txt", &hidden_prefix, EntryType::file(), 0, None),
            "prefix",
        ),
        (
            "gnu-layout",
            raw_gnu_header(&format!("{outside_prefix}/gnu.txt"), EntryType::file(), 0),
            "header dialect",
        ),
    ];

    for (index, (name, header, reason)) in cases.into_iter().enumerate() {
        let home = TestHome::new(&format!("skills-tar-path-canonical-{index}"));
        let archive = gzip_tar_records(
            vec![
                regular_record(&candidate_path, valid_manifest("review")),
                (header, Vec::new()),
            ],
            2,
            &[],
        );
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
        let error = resolve_source(
            SkillSourceInput::Github {
                value: "https://github.com/acme/skills/tree/main/catalog".into(),
            },
            server.endpoints(),
        )
        .unwrap_err();
        assert!(
            matches!(error, SkillError::InvalidSource { ref message } if message.contains(reason)),
            "{name} was not rejected by raw path preflight: {error:?}"
        );
        drop(server);
        drop(home);
    }
}

#[test]
fn full_width_ustar_name_and_prefix_fields_match_the_high_level_parser() {
    let _home = TestHome::new("skills-tar-full-width-paths");
    let candidate = regular_record(
        &format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
        valid_manifest("review"),
    );
    let full_name = vec![b'n'; 100];
    let name_prefix = format!("skills-{FIXTURE_SHA}/outside");
    let full_name_record = (
        raw_ustar_header(
            &full_name,
            name_prefix.as_bytes(),
            EntryType::file(),
            0,
            None,
        ),
        Vec::new(),
    );
    let mut full_prefix = format!("skills-{FIXTURE_SHA}/outside/").into_bytes();
    full_prefix.resize(155, b'p');
    let full_prefix_record = (
        raw_ustar_header(b"boundary.txt", &full_prefix, EntryType::file(), 0, None),
        Vec::new(),
    );
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(gzip_tar_records(
        vec![candidate, full_name_record, full_prefix_record],
        2,
        &[],
    )));

    let result = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills/tree/main/catalog".into(),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_eq!(
        result
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>(),
        vec!["review"]
    );
}

#[test]
fn malformed_tar_checksum_size_and_end_framing_are_rejected() {
    let path = format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md");
    let body = valid_manifest("review");

    let mut bad_checksum = raw_header(&path, EntryType::file(), body.len() as u64, None);
    bad_checksum.as_mut_bytes()[0] ^= 1;

    let mut bad_size = raw_header(&path, EntryType::file(), 0, None);
    bad_size.as_mut_bytes()[124..136].copy_from_slice(b"00000000009\0");
    bad_size.set_cksum();

    let mut embedded_nul_size = raw_header(&path, EntryType::file(), 0, None);
    embedded_nul_size.as_mut_bytes()[124..136].fill(0);
    embedded_nul_size.as_mut_bytes()[125..135].copy_from_slice(b"0000000001");
    embedded_nul_size.set_cksum();

    let trailing_nonzero = {
        let mut block = vec![0_u8; 512];
        block[0] = 1;
        block
    };
    let cases = vec![
        (
            "checksum",
            gzip_tar_records(vec![(bad_checksum, body.clone())], 2, &[]),
            "checksum",
        ),
        (
            "size",
            gzip_tar_records(vec![(bad_size, Vec::new())], 2, &[]),
            "size",
        ),
        (
            "embedded-nul-size",
            gzip_tar_records(vec![(embedded_nul_size, vec![0])], 2, &[]),
            "size",
        ),
        (
            "missing-end",
            gzip_tar_records(vec![regular_record(&path, body.clone())], 0, &[]),
            "end marker",
        ),
        (
            "torn-end",
            gzip_tar_records(vec![regular_record(&path, body.clone())], 1, &[]),
            "end marker",
        ),
        (
            "data-after-end",
            gzip_tar_records(vec![regular_record(&path, body)], 2, &trailing_nonzero),
            "trailing",
        ),
    ];

    for (index, (name, archive, reason)) in cases.into_iter().enumerate() {
        let home = TestHome::new(&format!("skills-tar-framing-{index}"));
        let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
        let error = resolve_source(
            SkillSourceInput::Github {
                value: "acme/skills".into(),
            },
            server.endpoints(),
        )
        .unwrap_err();
        assert!(
            matches!(error, SkillError::InvalidSource { ref message } if message.contains(reason)),
            "{name} did not report the raw framing failure: {error:?}"
        );
        drop(server);
        drop(home);
    }
}

#[cfg(unix)]
#[test]
fn github_success_retains_only_private_candidate_snapshots() {
    use std::os::unix::fs::PermissionsExt;

    let th = TestHome::new("skills-private-staging");
    let server = MockGithub::start(&["review"]);
    let result = resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into(),
        },
        server.endpoints(),
    )
    .unwrap();
    let operation = th
        .home
        .join(format!(".arca/staging/skills/{}", result.operation_id));
    for directory in [
        operation.clone(),
        operation.join("candidates"),
        operation.join("candidates/review"),
    ] {
        assert_eq!(
            fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
            0o700,
            "directory was not private: {}",
            directory.display()
        );
    }
    let candidate = operation.join("candidates/review/SKILL.md");
    let resolution = operation.join("resolution.json");
    for file in [candidate, resolution] {
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600,
            "file was not private: {}",
            file.display()
        );
    }
    for transient in [
        operation.join("source.tar.gz"),
        operation.join("source.tar"),
        operation.join("archive"),
    ] {
        assert!(
            !transient.exists(),
            "successful GitHub staging retained transient data: {}",
            transient.display()
        );
    }
    let mut retained = fs::read_dir(&operation)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    retained.sort();
    assert_eq!(
        retained,
        vec!["candidates", "metadata.json", "resolution.json"]
    );
}

#[cfg(unix)]
#[test]
fn local_candidate_snapshots_are_private_even_when_the_source_is_broad() {
    use std::os::unix::fs::PermissionsExt;

    let th = TestHome::new("skills-private-local-candidate");
    let source = th.home.join("source/review");
    write_skill(&source, "review", "review fixture");
    fs::create_dir(source.join("scripts")).unwrap();
    fs::write(source.join("scripts/run.sh"), b"#!/bin/sh\n").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o777)).unwrap();
    fs::set_permissions(source.join("SKILL.md"), fs::Permissions::from_mode(0o666)).unwrap();
    fs::set_permissions(
        source.join("scripts/run.sh"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    let result = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    )
    .unwrap();
    let candidate = th.home.join(format!(
        ".arca/staging/skills/{}/candidates/review",
        result.operation_id
    ));
    assert_eq!(
        fs::metadata(&candidate).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(candidate.join("scripts"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(candidate.join("SKILL.md"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(candidate.join("scripts/run.sh"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o711
    );
}

#[test]
fn github_rate_limited_403_is_retryable() {
    assert_status_behavior(
        "skills-http-rate-403",
        ArchiveBehavior::CommitStatus {
            status: "403 Forbidden",
            headers: vec![
                ("X-RateLimit-Remaining", "0"),
                ("X-RateLimit-Reset", "1893456000"),
            ],
            body: b"/private/rate/body",
        },
        "network",
        Some("2030-01-01T00:00:00Z"),
    );
}

#[test]
fn github_auth_403_remains_an_invalid_public_source() {
    assert_status_behavior(
        "skills-http-auth-403",
        ArchiveBehavior::CommitStatus {
            status: "403 Forbidden",
            headers: vec![],
            body: b"/private/auth/body",
        },
        "invalid_source",
        None,
    );
}

#[test]
fn github_429_uses_the_bounded_reset_header_as_retry_time() {
    assert_status_behavior(
        "skills-http-rate-429",
        ArchiveBehavior::CommitStatus {
            status: "429 Too Many Requests",
            headers: vec![("X-RateLimit-Reset", "1893456001")],
            body: b"/private/rate/body",
        },
        "network",
        Some("2030-01-01T00:00:01Z"),
    );
}

#[test]
fn github_reset_time_precedes_retry_after_fallback() {
    assert_status_behavior(
        "skills-http-rate-reset-precedence",
        ArchiveBehavior::CommitStatus {
            status: "429 Too Many Requests",
            headers: vec![("Retry-After", "120"), ("X-RateLimit-Reset", "1893456000")],
            body: b"/private/rate/body",
        },
        "network",
        Some("2030-01-01T00:00:00Z"),
    );
}

#[test]
fn github_rate_limited_403_rejects_invalid_or_out_of_range_reset_retry_times() {
    for (home_name, reset) in [
        ("skills-http-rate-invalid-reset", "12invalid"),
        ("skills-http-rate-out-of-range-reset", "253402300800"),
    ] {
        assert_status_behavior(
            home_name,
            ArchiveBehavior::CommitStatus {
                status: "403 Forbidden",
                headers: vec![("X-RateLimit-Remaining", "0"), ("X-RateLimit-Reset", reset)],
                body: b"/private/rate/body",
            },
            "network",
            None,
        );
    }
}

#[test]
fn github_5xx_is_a_retryable_network_error() {
    assert_status_behavior(
        "skills-http-server-500",
        ArchiveBehavior::CommitStatus {
            status: "500 Internal Server Error",
            headers: vec![],
            body: b"/private/server/body",
        },
        "network",
        None,
    );
}

fn assert_status_behavior(
    home_name: &str,
    behavior: ArchiveBehavior,
    expected_code: &str,
    expected_retry: Option<&str>,
) {
    let _home = TestHome::new(home_name);
    let server = ScriptedGithub::new(behavior);
    let error = resolve_source(
        SkillSourceInput::Github {
            value: "acme/skills".into(),
        },
        server.endpoints(),
    )
    .unwrap_err();
    let (code, retry_at) = match &error {
        SkillError::Network { retry_at, .. } => ("network", retry_at.as_deref()),
        SkillError::InvalidSource { .. } => ("invalid_source", None),
        other => panic!("unexpected status error: {other:?}"),
    };
    assert_eq!(
        code, expected_code,
        "wrong status classification: {error:?}"
    );
    assert_eq!(retry_at, expected_retry, "wrong retry time: {error:?}");
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains("/private/"));
    assert!(rendered.len() <= 1024);
}

#[test]
fn requested_subtree_is_the_only_place_scanned_for_candidates() {
    let _th = TestHome::new("skills-subtree-scan");
    let archive = archive_with(&[
        ArchiveEntry::File(
            format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
            valid_manifest("review"),
        ),
        ArchiveEntry::File(
            format!("skills-{FIXTURE_SHA}/outside/not-a-skill/SKILL.md"),
            b"not valid frontmatter".to_vec(),
        ),
    ]);
    let server = ScriptedGithub::new(ArchiveBehavior::Bytes(archive));
    let result = resolve_source(
        SkillSourceInput::Github {
            value: "https://github.com/acme/skills/tree/main/catalog".into(),
        },
        server.endpoints(),
    )
    .unwrap();
    assert_eq!(result.candidates.len(), 1);
    assert_eq!(result.candidates[0].name, "review");
}

#[test]
fn resolver_errors_hide_staging_paths_and_cap_messages() {
    let th = TestHome::new("skills-error-privacy");
    let source = th.home.join("invalid");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("SKILL.md"), "invalid").unwrap();
    let error = resolve_source(
        SkillSourceInput::Local {
            path: source.display().to_string(),
        },
        GithubEndpoints::production(),
    )
    .unwrap_err();
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(!rendered.contains(th.home.to_string_lossy().as_ref()));
    assert!(!rendered.contains("staging/skills"));
    match error {
        SkillError::InvalidManifest { message, path }
        | SkillError::UnsafePath { message, path }
        | SkillError::Conflict { message, path } => {
            assert!(message.chars().count() <= 512);
            assert!(path.is_empty());
        }
        SkillError::Io { message, path } => {
            assert!(message.chars().count() <= 512);
            assert!(path.is_none());
        }
        other => panic!("unexpected private error shape: {other:?}"),
    }
}

#[test]
fn test_endpoints_only_accept_loopback_http() {
    let result = std::panic::catch_unwind(|| {
        GithubEndpoints::for_test(
            Url::parse("http://example.com/").unwrap(),
            Url::parse("http://example.com/").unwrap(),
        )
    });
    assert!(result.is_err());
}

enum ArchiveEntry {
    File(String, Vec<u8>),
    RawFile(String, Vec<u8>),
    Directory(String),
    Link {
        path: String,
        target: String,
        kind: EntryType,
    },
}

fn valid_manifest(name: &str) -> Vec<u8> {
    format!("---\nname: {name}\ndescription: {name} fixture\n---\nbody\n").into_bytes()
}

fn valid_archive(names: &[&str]) -> Vec<u8> {
    let entries = names
        .iter()
        .map(|name| {
            ArchiveEntry::File(
                format!("skills-{FIXTURE_SHA}/catalog/{name}/SKILL.md"),
                valid_manifest(name),
            )
        })
        .collect::<Vec<_>>();
    archive_with(&entries)
}

fn archive_with(entries: &[ArchiveEntry]) -> Vec<u8> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut builder = Builder::new(encoder);
    for entry in entries {
        match entry {
            ArchiveEntry::File(path, body) => {
                let mut header = file_header(body.len() as u64);
                builder
                    .append_data(&mut header, path, Cursor::new(body))
                    .unwrap();
            }
            ArchiveEntry::RawFile(path, body) => {
                let mut header = raw_header(path, EntryType::file(), body.len() as u64, None);
                header.set_cksum();
                builder.append(&header, Cursor::new(body)).unwrap();
            }
            ArchiveEntry::Directory(path) => {
                let mut header = Header::new_ustar();
                header.set_entry_type(EntryType::dir());
                header.set_mode(0o755);
                header.set_size(0);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, Cursor::new(Vec::<u8>::new()))
                    .unwrap();
            }
            ArchiveEntry::Link { path, target, kind } => {
                let mut header = Header::new_ustar();
                header.set_entry_type(*kind);
                header.set_mode(0o777);
                header.set_size(0);
                if !target.is_empty() {
                    header.set_link_name(target).unwrap();
                }
                header.set_cksum();
                builder
                    .append_data(&mut header, path, Cursor::new(Vec::<u8>::new()))
                    .unwrap();
            }
        }
    }
    let encoder = builder.into_inner().unwrap();
    encoder.finish().unwrap()
}

fn file_header(size: u64) -> Header {
    let mut header = Header::new_ustar();
    header.set_mode(0o644);
    header.set_size(size);
    header.set_cksum();
    header
}

fn raw_header(path: &str, kind: EntryType, size: u64, link: Option<&str>) -> Header {
    assert!(path.len() < 100);
    raw_ustar_header(path.as_bytes(), &[], kind, size, link)
}

fn raw_ustar_header(
    name: &[u8],
    prefix: &[u8],
    kind: EntryType,
    size: u64,
    link: Option<&str>,
) -> Header {
    assert!(name.len() <= 100);
    assert!(prefix.len() <= 155);
    let mut header = Header::new_ustar();
    header.set_entry_type(kind);
    header.set_mode(0o644);
    header.set_size(size);
    if let Some(link) = link {
        header.set_link_name(link).unwrap();
    }
    let bytes = header.as_mut_bytes();
    bytes[..100].fill(0);
    bytes[..name.len()].copy_from_slice(name);
    bytes[345..500].fill(0);
    bytes[345..345 + prefix.len()].copy_from_slice(prefix);
    header.set_cksum();
    header
}

fn raw_gnu_header(path: &str, kind: EntryType, size: u64) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_mode(0o644);
    header.set_size(size);
    header.set_path(path).unwrap();
    header.set_cksum();
    header
}

fn gzip_raw_tar(headers: &[Header]) -> Vec<u8> {
    let mut tar = Vec::new();
    for header in headers {
        tar.extend_from_slice(header.as_bytes());
    }
    tar.extend_from_slice(&[0_u8; 1024]);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

fn gzip_tar_records(
    records: Vec<(Header, Vec<u8>)>,
    end_blocks: usize,
    trailing: &[u8],
) -> Vec<u8> {
    let mut tar = Vec::new();
    for (header, body) in records {
        tar.extend_from_slice(header.as_bytes());
        tar.extend_from_slice(&body);
        let padding = (512 - body.len() % 512) % 512;
        tar.resize(tar.len() + padding, 0);
    }
    tar.resize(tar.len() + end_blocks * 512, 0);
    tar.extend_from_slice(trailing);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&tar).unwrap();
    encoder.finish().unwrap()
}

fn regular_record(path: &str, body: Vec<u8>) -> (Header, Vec<u8>) {
    (
        raw_header(path, EntryType::file(), body.len() as u64, None),
        body,
    )
}

fn extension_record(kind: u8, body: Vec<u8>) -> (Header, Vec<u8>) {
    (
        raw_header(
            "././@ArcaExtension",
            EntryType::new(kind),
            body.len() as u64,
            None,
        ),
        body,
    )
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let payload = format!("{key}={value}\n");
    let mut length = payload.len() + 2;
    loop {
        let next = length.to_string().len() + 1 + payload.len();
        if next == length {
            return format!("{length} {payload}").into_bytes();
        }
        length = next;
    }
}

fn physical_limit_records() -> Vec<(Header, Vec<u8>)> {
    let mut records = Vec::with_capacity(MAX_ARCHIVE_ENTRIES as usize);
    records.push(regular_record(
        &format!("skills-{FIXTURE_SHA}/catalog/review/SKILL.md"),
        valid_manifest("review"),
    ));
    for index in 0..MAX_ARCHIVE_ENTRIES - 1 {
        records.push((
            raw_header(
                &format!("skills-{FIXTURE_SHA}/empty/{index}"),
                EntryType::dir(),
                0,
                None,
            ),
            Vec::new(),
        ));
    }
    records
}

fn gzip_large_extension(size: u64) -> Vec<u8> {
    let header = raw_header("././@LongLink", EntryType::new(b'L'), size, None);
    let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(header.as_bytes()).unwrap();
    std::io::copy(&mut std::io::repeat(0).take(size), &mut encoder).unwrap();
    let padding = (512 - size % 512) % 512;
    std::io::copy(&mut std::io::repeat(0).take(padding + 1024), &mut encoder).unwrap();
    encoder.finish().unwrap()
}

enum ArchiveBehavior {
    Bytes(Vec<u8>),
    Paused {
        archive: Vec<u8>,
        reached: mpsc::SyncSender<()>,
        resume: mpsc::Receiver<()>,
    },
    Chunked(u64),
    Redirects {
        count: usize,
        archive: Vec<u8>,
    },
    CommitStatus {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: &'static [u8],
    },
}

struct ScriptedGithub {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ScriptedGithub {
    fn new(behavior: ArchiveBehavior) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !thread_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => scripted_connection(stream, &behavior),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            address,
            stop,
            thread: Some(thread),
        }
    }

    fn endpoints(&self) -> GithubEndpoints {
        let base = Url::parse(&format!("http://{}/", self.address)).unwrap();
        GithubEndpoints::for_test(base.clone(), base)
    }

    fn paused_archive(archive: Vec<u8>) -> (Self, mpsc::Receiver<()>, mpsc::SyncSender<()>) {
        let (reached_tx, reached_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        (
            Self::new(ArchiveBehavior::Paused {
                archive,
                reached: reached_tx,
                resume: resume_rx,
            }),
            reached_rx,
            resume_tx,
        )
    }
}

impl Drop for ScriptedGithub {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let _ = TcpStream::connect_timeout(&self.address, Duration::from_millis(20));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn scripted_connection(mut stream: TcpStream, behavior: &ArchiveBehavior) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    while !request.windows(4).any(|window| window == b"\r\n\r\n") && request.len() < 64 * 1024 {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => request.extend_from_slice(&buffer[..read]),
        }
    }
    let request = String::from_utf8_lossy(&request);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = target.split('?').next().unwrap_or(target);
    if path == "/repos/acme/skills" {
        response(&mut stream, "200 OK", &[], br#"{"default_branch":"main"}"#);
    } else if path == "/repos/acme/skills/commits/main"
        || path == format!("/repos/acme/skills/commits/{FIXTURE_SHA}")
    {
        if let ArchiveBehavior::CommitStatus {
            status,
            headers,
            body,
        } = behavior
        {
            response(&mut stream, status, headers, body);
        } else {
            response(
                &mut stream,
                "200 OK",
                &[],
                format!(r#"{{"sha":"{FIXTURE_SHA}"}}"#).as_bytes(),
            );
        }
    } else if path.starts_with("/repos/acme/skills/commits/") {
        response(&mut stream, "404 Not Found", &[], b"");
    } else if path == format!("/acme/skills/tar.gz/{FIXTURE_SHA}") || path.starts_with("/hop/") {
        match behavior {
            ArchiveBehavior::Bytes(bytes) => response(&mut stream, "200 OK", &[], bytes),
            ArchiveBehavior::Paused {
                archive,
                reached,
                resume,
            } => {
                if reached.send(()).is_ok() && resume.recv_timeout(Duration::from_secs(5)).is_ok() {
                    response(&mut stream, "200 OK", &[], archive);
                }
            }
            ArchiveBehavior::Chunked(total) => chunked_response(&mut stream, *total),
            ArchiveBehavior::Redirects { count, archive } => {
                let current = path
                    .strip_prefix("/hop/")
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                if current < *count {
                    let next = format!("/hop/{}", current + 1);
                    response(&mut stream, "302 Found", &[("Location", &next)], b"");
                } else {
                    response(&mut stream, "200 OK", &[], archive);
                }
            }
            ArchiveBehavior::CommitStatus { .. } => {
                response(&mut stream, "500 Internal Server Error", &[], b"");
            }
        }
    } else {
        response(&mut stream, "404 Not Found", &[], b"");
    }
}

fn response(stream: &mut TcpStream, status: &str, headers: &[(&str, &str)], body: &[u8]) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body);
}

fn chunked_response(stream: &mut TcpStream, total: u64) {
    let _ = stream
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
    let chunk = [0_u8; 64 * 1024];
    let mut written = 0_u64;
    while written < total {
        let size = (total - written).min(chunk.len() as u64) as usize;
        if stream
            .write_all(format!("{size:x}\r\n").as_bytes())
            .and_then(|_| stream.write_all(&chunk[..size]))
            .and_then(|_| stream.write_all(b"\r\n"))
            .is_err()
        {
            return;
        }
        written += size as u64;
    }
    let _ = stream.write_all(b"0\r\n\r\n");
}

fn assert_private_directory(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
