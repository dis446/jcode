use super::*;
use std::fs;
use std::sync::atomic::AtomicBool;

fn cand(path: &str) -> Candidate {
    let is_dir = path.ends_with('/');
    let trimmed = path.trim_end_matches('/');
    Candidate {
        path: path.to_string(),
        name: trimmed.rsplit('/').next().unwrap().to_string(),
        is_dir,
    }
}

fn prefix(text: &str) -> Option<(usize, String, bool)> {
    extract_at_prefix(text, text.len()).map(|p| (p.start, p.query, p.quoted))
}

#[test]
fn prefix_triggers() {
    assert_eq!(prefix("@"), Some((0, String::new(), false)));
    assert_eq!(prefix("@rea"), Some((0, "rea".into(), false)));
    assert_eq!(prefix("look at @src/ma"), Some((8, "src/ma".into(), false)));
    assert_eq!(prefix("(@x"), Some((1, "x".into(), false)));
    assert_eq!(prefix("`@x"), Some((1, "x".into(), false)));
    assert_eq!(prefix("see:\n@a"), Some((5, "a".into(), false)));
    assert_eq!(prefix("\u{3002}@a"), Some((3, "a".into(), false)));
}

#[test]
fn prefix_rejects() {
    assert_eq!(prefix("email@x.com"), None);
    assert_eq!(prefix("hello"), None);
    assert_eq!(prefix("@a b"), None);
    assert_eq!(prefix(""), None);
}

#[test]
fn prefix_quoted() {
    assert_eq!(prefix("@\"my fi"), Some((0, "my fi".into(), true)));
    assert_eq!(prefix("x @\""), Some((2, String::new(), true)));
    assert_eq!(prefix("@\"done\" more"), None);
    assert_eq!(prefix("a@\"x"), None);
}

#[test]
fn prefix_mid_text_uses_cursor() {
    let text = "@rea tail";
    let p = extract_at_prefix(text, 4).unwrap();
    assert_eq!((p.start, p.query.as_str()), (0, "rea"));
}

#[test]
fn scoring() {
    assert_eq!(score_entry(&cand("README.md"), "readme.md"), Some(100));
    assert_eq!(score_entry(&cand("README.md"), "rea"), Some(80));
    assert_eq!(score_entry(&cand("docs/README.md"), "adme"), Some(50));
    assert_eq!(score_entry(&cand("src/"), "src"), Some(110));
    assert_eq!(score_entry(&cand("src/lib/a.rs"), "lib/a"), Some(30));
    assert_eq!(score_entry(&cand("a.rs"), "zzz"), None);
    assert_eq!(score_entry(&cand("a.rs"), ""), Some(1));
}

#[test]
fn ranking_order() {
    let ranked = rank(
        vec![
            cand("docs/readme.md"),
            cand("readme.md"),
            cand("xreadme/"),
            cand("README.txt"),
        ],
        "readme",
    );
    let paths: Vec<_> = ranked.iter().map(|c| c.path.as_str()).collect();
    // exact(100), starts-with(80) by depth then len (README.txt before docs/readme.md),
    // then the dir at contains 50 + 10.
    assert_eq!(paths, ["readme.md", "README.txt", "docs/readme.md", "xreadme/"]);
}

#[test]
fn ranking_truncates() {
    let items: Vec<_> = (0..50).map(|i| cand(&format!("f{i:02}.rs"))).collect();
    assert_eq!(rank(items, "f").len(), MAX_RESULTS);
}

#[test]
fn completion_file_and_dir() {
    let p = extract_at_prefix("see @sr", 7).unwrap();
    let (t, c) = apply_completion("see @sr", 7, &p, &cand("src/"));
    assert_eq!((t.as_str(), c), ("see @src/", 9));
    let (t, c) = apply_completion("see @sr", 7, &p, &cand("src/main.rs"));
    assert_eq!((t.as_str(), c), ("see @src/main.rs ", 17));
}

#[test]
fn completion_quotes() {
    let p = extract_at_prefix("@\"my", 4).unwrap();
    let (t, c) = apply_completion("@\"my", 4, &p, &cand("my dir/"));
    assert_eq!(t, "@\"my dir/\"");
    assert_eq!(c, t.len() - 1);
    let (t, c) = apply_completion("@\"my", 4, &p, &cand("my file.txt"));
    assert_eq!(t, "@\"my file.txt\" ");
    assert_eq!(c, t.len());
    // Spaces in the path force quotes even when the prefix was plain.
    let p = extract_at_prefix("@my", 3).unwrap();
    let (t, _) = apply_completion("@my", 3, &p, &cand("my file.txt"));
    assert_eq!(t, "@\"my file.txt\" ");
}

#[test]
fn completion_reuses_existing_closing_quote() {
    let text = "@\"my\" rest";
    let p = extract_at_prefix(text, 4).unwrap();
    let (t, _) = apply_completion(text, 4, &p, &cand("my file.txt"));
    assert_eq!(t, "@\"my file.txt\"  rest");
}

fn tree() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let r = d.path();
    fs::create_dir_all(r.join("src/deep")).unwrap();
    fs::create_dir_all(r.join(".git")).unwrap();
    fs::create_dir_all(r.join("target")).unwrap();
    fs::create_dir_all(r.join("real")).unwrap();
    fs::write(r.join("README.md"), "").unwrap();
    fs::write(r.join(".env"), "").unwrap();
    fs::write(r.join(".git/config"), "").unwrap();
    fs::write(r.join("target/junk.rs"), "").unwrap();
    fs::write(r.join("src/main.rs"), "").unwrap();
    fs::write(r.join("src/deep/mod.rs"), "").unwrap();
    fs::write(r.join("real/inner.txt"), "").unwrap();
    fs::write(r.join(".gitignore"), "target/\n").unwrap();
    // `ignore` only reads .gitignore inside a git work tree.
    fs::create_dir_all(r.join(".git/refs")).unwrap();
    fs::write(r.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(r.join("real"), r.join("link")).unwrap();
    d
}

fn run(d: &tempfile::TempDir, q: &str) -> Vec<String> {
    let cancel = AtomicBool::new(false);
    search(d.path(), q, &cancel)
        .into_iter()
        .map(|c| c.path)
        .collect()
}

#[test]
fn walker_basics() {
    let d = tree();
    let r = run(&d, "readm");
    assert_eq!(r.first().map(String::as_str), Some("README.md"));
    // dir bonus: `real/` (80+10) outranks README.md (80) for "rea", as in pi.
    assert_eq!(run(&d, "rea").first().map(String::as_str), Some("real/"));
    let all = run(&d, "");
    assert!(all.contains(&".env".to_string()), "hidden included");
    assert!(!all.iter().any(|p| p.starts_with(".git/") || p == ".git/"), ".git excluded");
    assert!(!all.iter().any(|p| p.starts_with("target/")), "gitignore respected");
    assert!(all.contains(&"src/deep/mod.rs".to_string()), "recursive");
}

#[test]
fn walker_scoped() {
    let d = tree();
    let r = run(&d, "src/");
    assert!(r.contains(&"src/main.rs".to_string()));
    assert!(r.contains(&"src/deep/".to_string()));
    assert!(!r.contains(&"README.md".to_string()));
    let r = run(&d, "src/ma");
    assert_eq!(r, vec!["src/main.rs".to_string()]);
}

#[test]
fn walker_unscoped_path_query_falls_back_to_full_path() {
    let d = tree();
    let r = run(&d, "nope/deep");
    assert!(r.is_empty());
    let r = run(&d, "deep/mo");
    assert_eq!(r, vec!["src/deep/mod.rs".to_string()]);
}

#[cfg(unix)]
#[test]
fn walker_follows_symlinked_dir() {
    let d = tree();
    let r = run(&d, "inner");
    assert!(r.contains(&"link/inner.txt".to_string()), "{r:?}");
}

#[test]
fn walker_absolute_scope() {
    let d = tree();
    let abs = format!("{}/src/", d.path().display());
    let r = run(&d, &abs);
    assert!(r.iter().any(|p| p.ends_with("src/main.rs")));
}

#[test]
fn walker_cancel_returns_quickly() {
    let d = tree();
    let cancel = AtomicBool::new(true);
    assert!(search(d.path(), "", &cancel).len() <= 1);
}
