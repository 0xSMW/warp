use virtual_fs::{Stub, VirtualFS};
use warp_util::local_or_remote_path::LocalOrRemotePath;

use super::{
    extract_skill_parent_directory, find_local_project_skill_files_on_filesystem,
    is_home_provider_path, is_home_skill_directory, is_skill_file, read_skills_from_files,
};

#[test]
fn is_skill_file_valid_paths() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills")
        .join("my-skill")
        .join("SKILL.md");
    assert!(is_skill_file(&path));

    let path = home_dir
        .join(".claude")
        .join("skills")
        .join("test-skill")
        .join("SKILL.md");
    assert!(is_skill_file(&path));

    // Path with multiple levels of prefix
    let path = home_dir
        .join("very")
        .join("deep")
        .join("path")
        .join("to")
        .join("repo")
        .join(".agents")
        .join("skills")
        .join("another-skill")
        .join("SKILL.md");
    assert!(is_skill_file(&path));
}

#[test]
fn is_skill_file_invalid_provider() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    let path = home_dir
        .join("repos")
        .join("project")
        .join(".garbage")
        .join("skills")
        .join("my-skill")
        .join("SKILL.md");
    assert!(!is_skill_file(&path));

    let path = home_dir
        .join(".garbage")
        .join("skills")
        .join("test-skill")
        .join("SKILL.md");
    assert!(!is_skill_file(&path));
}

#[test]
fn is_skill_file_invalid_format() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // Missing SKILL.md at the end
    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills")
        .join("my-skill")
        .join("README.md");
    assert!(!is_skill_file(&path));

    // No skill name directory
    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills")
        .join("SKILL.md");
    assert!(!is_skill_file(&path));

    // Extra directory level in skill name
    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills")
        .join("nested")
        .join("my-skill")
        .join("SKILL.md");
    assert!(!is_skill_file(&path));

    // Missing provider directory
    let path = home_dir
        .join("repos")
        .join("project")
        .join("skills")
        .join("my-skill")
        .join("SKILL.md");
    assert!(!is_skill_file(&path));

    // Plain file path
    let path = home_dir.join("some").join("random").join("file.txt");
    assert!(!is_skill_file(&path));
}

// ============================================================================
// Tests for extract_skill_parent_directory
// ============================================================================

#[test]
fn extract_skill_parent_directory_from_repo_root() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let parent_directory = home_dir.join("repo");
    let skill_path = parent_directory
        .join(".agents")
        .join("skills")
        .join("my-skill")
        .join("SKILL.md");
    let result = extract_skill_parent_directory(&LocalOrRemotePath::Local(skill_path));
    assert_eq!(
        result.ok(),
        Some(LocalOrRemotePath::Local(parent_directory))
    );
}

#[test]
fn extract_skill_parent_directory_from_subdirectory() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let parent_directory = home_dir.join("repo").join("packages").join("frontend");
    let skill_path = parent_directory
        .join(".agents")
        .join("skills")
        .join("build")
        .join("SKILL.md");
    let result = extract_skill_parent_directory(&LocalOrRemotePath::Local(skill_path));
    assert_eq!(
        result.ok(),
        Some(LocalOrRemotePath::Local(parent_directory))
    );
}

#[test]
fn extract_skill_parent_directory_from_deep_subdirectory() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let parent_directory = home_dir
        .join("repo")
        .join("a")
        .join("b")
        .join("c")
        .join("d");
    let skill_path = parent_directory
        .join(".claude")
        .join("skills")
        .join("test-skill")
        .join("SKILL.md");
    let result = extract_skill_parent_directory(&LocalOrRemotePath::Local(skill_path.clone()));
    assert_eq!(
        result.ok(),
        Some(LocalOrRemotePath::Local(parent_directory)),
        "Failed for path: {}",
        skill_path.display()
    );
}

#[test]
fn extract_skill_parent_directory_different_providers() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let repo = home_dir.join("repo");
    let providers = [".warp", ".claude", ".codex", ".cursor", ".gemini"];
    for provider in providers {
        let path = repo
            .join(provider)
            .join("skills")
            .join("s")
            .join("SKILL.md");
        let result = extract_skill_parent_directory(&LocalOrRemotePath::Local(path.clone()));
        assert_eq!(
            result.ok(),
            Some(LocalOrRemotePath::Local(repo.clone())),
            "Failed for path: {}",
            path.display()
        );
    }
}

#[test]
fn extract_skill_parent_directory_returns_none_for_non_skill() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // Not a SKILL.md file
    let path = home_dir
        .join("repo")
        .join(".agents")
        .join("skills")
        .join("my-skill")
        .join("README.md");
    assert_eq!(
        extract_skill_parent_directory(&LocalOrRemotePath::Local(path)).ok(),
        None
    );

    // Wrong structure (skill directly in skills dir)
    let path = home_dir
        .join("repo")
        .join(".agents")
        .join("skills")
        .join("SKILL.md");
    assert_eq!(
        extract_skill_parent_directory(&LocalOrRemotePath::Local(path)).ok(),
        None
    );

    // Too deeply nested
    let path = home_dir
        .join("repo")
        .join(".agents")
        .join("skills")
        .join("a")
        .join("b")
        .join("SKILL.md");
    assert_eq!(
        extract_skill_parent_directory(&LocalOrRemotePath::Local(path)).ok(),
        None
    );

    // Not in a skills directory
    let path = home_dir.join("repo").join("src").join("SKILL.md");
    assert_eq!(
        extract_skill_parent_directory(&LocalOrRemotePath::Local(path)).ok(),
        None
    );
}

// ============================================================================
// Tests for is_home_skill_directory
// ============================================================================

#[test]
fn is_home_skill_directory_true_for_home_skill_dir() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // ~/.agents/skills/skill-name
    let path = home_dir.join(".agents").join("skills").join("my-skill");
    assert!(is_home_skill_directory(&path));

    // ~/.claude/skills/skill-name
    let path = home_dir.join(".claude").join("skills").join("test-skill");
    assert!(is_home_skill_directory(&path));
}

#[test]
fn is_home_skill_directory_false_for_project_skill_dir() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // ~/repos/project/.agents/skills/my-skill is a project skill dir, not home
    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills")
        .join("my-skill");
    assert!(!is_home_skill_directory(&path));
}

#[test]
fn is_home_skill_directory_false_for_provider_path_itself() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // ~/.agents/skills is the provider path, not a skill directory
    let path = home_dir.join(".agents").join("skills");
    assert!(!is_home_skill_directory(&path));
}

#[test]
fn is_home_skill_directory_false_for_arbitrary_path() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    let path = home_dir.join("some").join("random").join("dir");
    assert!(!is_home_skill_directory(&path));
}

// ============================================================================
// Tests for is_home_provider_path
// ============================================================================

#[test]
fn is_home_provider_path_true_for_known_providers() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    let path = home_dir.join(".agents").join("skills");
    assert!(is_home_provider_path(&path));

    if let Some(path) = warp_core::paths::warp_home_skills_dir() {
        assert!(is_home_provider_path(&path));
    }

    let path = home_dir.join(".claude").join("skills");
    assert!(is_home_provider_path(&path));

    let path = home_dir.join(".codex").join("skills");
    assert!(is_home_provider_path(&path));

    let path = home_dir.join(".cursor").join("skills");
    assert!(is_home_provider_path(&path));

    let path = home_dir.join(".gemini").join("skills");
    assert!(is_home_provider_path(&path));
}

#[test]
fn extract_skill_parent_directory_returns_home_dir_for_warp_home_skill() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let Some(warp_home_skills_dir) = warp_core::paths::warp_home_skills_dir() else {
        eprintln!("Skipping test: Warp home skills directory not available");
        return;
    };

    let skill_path = warp_home_skills_dir.join("test-skill").join("SKILL.md");
    let result = extract_skill_parent_directory(&LocalOrRemotePath::Local(skill_path));
    assert_eq!(result.ok(), Some(LocalOrRemotePath::Local(home_dir)));
}

#[test]
fn is_home_provider_path_false_for_unknown_provider() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    let path = home_dir.join(".garbage").join("skills");
    assert!(!is_home_provider_path(&path));
}

#[test]
fn is_home_provider_path_false_for_project_provider() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // Project-level provider path, not home
    let path = home_dir
        .join("repos")
        .join("project")
        .join(".agents")
        .join("skills");
    assert!(!is_home_provider_path(&path));
}

#[test]
fn is_home_provider_path_false_for_partial_path() {
    let Some(home_dir) = dirs::home_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };

    // Just the provider directory, not the skills subdirectory
    let path = home_dir.join(".agents");
    assert!(!is_home_provider_path(&path));

    // Just home
    assert!(!is_home_provider_path(&home_dir));
}

#[test]
fn local_discovery_finds_root_skills() {
    VirtualFS::test("find_root_skills", |dirs, mut vfs| {
        vfs.mkdir("repo/.agents/skills/root-skill-1")
            .mkdir("repo/.claude/skills/root-skill-2")
            .with_files(vec![
                Stub::FileWithContent(
                    "repo/.agents/skills/root-skill-1/SKILL.md",
                    "---\nname: root-skill-1\ndescription: test\n---\n# root-skill-1",
                ),
                Stub::FileWithContent(
                    "repo/.claude/skills/root-skill-2/SKILL.md",
                    "---\nname: root-skill-2\ndescription: test\n---\n# root-skill-2",
                ),
            ]);
        let repo = dirs.tests().join("repo");

        let files = find_local_project_skill_files_on_filesystem(&repo);

        assert_eq!(files.len(), 2);
        assert!(files.contains(&LocalOrRemotePath::Local(
            repo.join(".agents/skills/root-skill-1/SKILL.md")
        )));
        assert!(files.contains(&LocalOrRemotePath::Local(
            repo.join(".claude/skills/root-skill-2/SKILL.md")
        )));
        let skills = read_skills_from_files(
            files
                .into_iter()
                .map(|path| path.to_local_path().unwrap().to_path_buf()),
        );
        let names: Vec<_> = skills.iter().map(|skill| skill.name.as_str()).collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"root-skill-1"));
        assert!(names.contains(&"root-skill-2"));
    });
}

#[test]
fn local_discovery_finds_nested_skills() {
    VirtualFS::test("find_nested_skills", |dirs, mut vfs| {
        vfs.mkdir("repo/packages/frontend/.agents/skills/frontend")
            .with_files(vec![Stub::FileWithContent(
                "repo/packages/frontend/.agents/skills/frontend/SKILL.md",
                "test",
            )]);
        let repo = dirs.tests().join("repo");

        assert_eq!(
            find_local_project_skill_files_on_filesystem(&repo),
            vec![LocalOrRemotePath::Local(
                repo.join("packages/frontend/.agents/skills/frontend/SKILL.md")
            )]
        );
    });
}

#[test]
fn local_discovery_skips_git_metadata() {
    VirtualFS::test("find_skills_skips_git", |dirs, mut vfs| {
        vfs.mkdir("repo/.git/.agents/skills/internal")
            .with_files(vec![Stub::FileWithContent(
                "repo/.git/.agents/skills/internal/SKILL.md",
                "test",
            )]);

        assert!(
            find_local_project_skill_files_on_filesystem(&dirs.tests().join("repo")).is_empty()
        );
    });
}

#[cfg(unix)]
#[test]
fn local_discovery_resolves_symlinked_skill_directory() {
    VirtualFS::test("find_local_symlinked_skill", |dirs, mut vfs| {
        vfs.mkdir("repo/.agents/skills")
            .mkdir("target")
            .with_files(vec![Stub::FileWithContent("target/SKILL.md", "test")]);
        let repo = dirs.tests().join("repo");
        let linked = repo.join(".agents/skills/linked");
        std::os::unix::fs::symlink(dirs.tests().join("target"), &linked).unwrap();

        assert_eq!(
            find_local_project_skill_files_on_filesystem(&repo),
            vec![LocalOrRemotePath::Local(linked.join("SKILL.md"))]
        );
    });
}

#[test]
fn local_discovery_includes_gitignored_skills() {
    VirtualFS::test("find_gitignored_skills", |dirs, mut vfs| {
        vfs.mkdir("repo/.agents/skills/ignored").with_files(vec![
            Stub::FileWithContent("repo/.gitignore", ".agents/\n"),
            Stub::FileWithContent("repo/.agents/skills/ignored/SKILL.md", "test"),
        ]);
        let repo = dirs.tests().join("repo");

        assert_eq!(
            find_local_project_skill_files_on_filesystem(&repo),
            vec![LocalOrRemotePath::Local(
                repo.join(".agents/skills/ignored/SKILL.md")
            )]
        );
    });
}

#[test]
fn local_discovery_returns_no_skills_for_empty_repo() {
    VirtualFS::test("find_skills_empty", |dirs, mut vfs| {
        vfs.mkdir("repo/src");

        assert!(
            find_local_project_skill_files_on_filesystem(&dirs.tests().join("repo")).is_empty()
        );
    });
}
