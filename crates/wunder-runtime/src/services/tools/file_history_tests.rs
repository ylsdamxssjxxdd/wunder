//! `file_history` 的单元测试：游标栈、损坏快照、回收压实。

use super::*;

use super::*;

fn seed(workspace: &Path, name: &str, content: &str) -> PathBuf {
    let target = workspace.join(name);
    fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");
    fs::write(&target, content).expect("seed");
    target
}

fn read(target: &Path) -> String {
    fs::read_to_string(target).expect("read")
}

fn revs_of(workspace: &Path, name: &str) -> Vec<u32> {
    let (dir, file) = locate(&workspace.join(name), workspace).expect("locate");
    revs_in(&dir, &file)
}

/// 写一次文件：先压栈再落盘，和真实调用方的顺序一致。
fn commit(target: &Path, workspace: &Path, before: &str, after: &str) {
    snapshot(target, workspace, before).expect("snapshot");
    fs::write(target, after).expect("write");
}

#[test]
fn snapshots_increment_and_stay_under_temp_history() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = workspace.join("temp/pelican.py");
    fs::create_dir_all(target.parent().expect("parent")).expect("mkdir");

    let first = snapshot(&target, workspace, "v1\n").expect("first snapshot");
    assert_eq!(first.rev, 1);
    // 文件名带内容短 sha：读取时能核对内容有没有被换掉。
    assert_eq!(
        first.display,
        format!("temp/.history/pelican.py@1-{}", short_hash("v1\n"))
    );
    assert!(workspace.join(&first.display).exists());

    let second = snapshot(&target, workspace, "v2\n").expect("second snapshot");
    assert_eq!(second.rev, 2);
    assert_eq!(
        second.display,
        format!("temp/.history/pelican.py@2-{}", short_hash("v2\n"))
    );
    assert_eq!(
        fs::read_to_string(workspace.join(&second.display)).expect("read"),
        "v2\n"
    );
}

#[test]
fn outside_workspace_files_land_in_external_bucket() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path().join("ws");
    fs::create_dir_all(&workspace).expect("mkdir");
    let outside = root.path().join("elsewhere/sketch.py");

    let snap = snapshot(&outside, &workspace, "x = 1\n").expect("snapshot");
    assert!(snap.display.starts_with("temp/.history/_external/"));
    assert!(snap.display.contains("elsewhere"));
}

#[test]
fn pruning_keeps_the_newest_versions() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = workspace.join("a.txt");
    fs::create_dir_all(workspace.join("temp/.history")).expect("mkdir");
    for rev in 1..=24u32 {
        fs::write(
            workspace.join(format!("temp/.history/a.txt@{}", rev)),
            "old\n",
        )
        .expect("seed");
    }
    // 没有 @cursor 的旧目录：游标取最新一版（24），新写入落在 25 并淘汰到 20 版。
    let snap = snapshot(&target, workspace, "new\n").expect("snapshot");
    assert_eq!(snap.rev, 25);
    assert_eq!(revs_of(workspace, "a.txt").len(), KEEP_PER_FILE as usize);
}

#[test]
fn undo_walks_the_whole_stack_and_redo_walks_it_back() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/seq.py", "A\n");

    // A → B → C：栈里是 A、B，游标 2，文件是 C。
    commit(&target, workspace, "A\n", "B\n");
    commit(&target, workspace, "B\n", "C\n");

    // 撤销：C → B → A，不新增历史态。
    let first = step_back(&target, workspace, "C\n").expect("undo 1");
    assert_eq!(first.content, "B\n");
    assert_eq!(first.cursor, 1);
    fs::write(&target, &first.content).expect("write back");
    commit_cursor(&target, workspace, first.cursor).expect("cursor");
    assert_eq!(read(&target), "B\n");

    let second = step_back(&target, workspace, "B\n").expect("undo 2");
    assert_eq!(second.content, "A\n");
    assert_eq!(second.cursor, 0);
    fs::write(&target, &second.content).expect("write back");
    commit_cursor(&target, workspace, second.cursor).expect("cursor");
    assert_eq!(read(&target), "A\n");
    assert_eq!(
        step_back(&target, workspace, "A\n").unwrap_err(),
        // A、B 两态 + 补出来的 redo 态 C，一共 3 份快照文件。
        StepBlock::AtOldest { revisions: 3 },
        "栈底必须停住，并说明已经是最早版本"
    );

    // 重做原路返回：A → B → C，同样不新增历史态。
    let back = step_forward(&target, workspace).expect("redo 1");
    assert_eq!(back.content, "B\n");
    assert_eq!(back.cursor, 1);
    fs::write(&target, &back.content).expect("write back");
    commit_cursor(&target, workspace, back.cursor).expect("cursor");
    assert_eq!(read(&target), "B\n");

    let back = step_forward(&target, workspace).expect("redo 2");
    assert_eq!(back.content, "C\n");
    assert_eq!(back.cursor, 2);
    fs::write(&target, &back.content).expect("write back");
    commit_cursor(&target, workspace, back.cursor).expect("cursor");
    assert_eq!(read(&target), "C\n");
    assert_eq!(
        step_forward(&target, workspace).unwrap_err(),
        StepBlock::AtNewest,
        "栈顶必须停住"
    );
}

#[test]
fn writing_after_undo_drops_the_redo_branch() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/branch.py", "A\n");

    commit(&target, workspace, "A\n", "B\n");
    commit(&target, workspace, "B\n", "C\n");

    // 退回到 B：C 从此是可丢弃的 redo 分支。
    let back = step_back(&target, workspace, "C\n").expect("undo");
    assert_eq!(back.content, "B\n");
    fs::write(&target, &back.content).expect("restore B");
    commit_cursor(&target, workspace, back.cursor).expect("cursor");

    // 从 B 另起一版 D：redo 分支被砍掉，重做无处可去。
    commit(&target, workspace, "B\n", "D\n");
    assert_eq!(
        step_forward(&target, workspace).unwrap_err(),
        StepBlock::AtNewest,
        "分支应已丢弃"
    );
    assert_eq!(revs_of(workspace, "temp/branch.py"), vec![1, 2]);
}

#[test]
fn identical_content_is_not_recorded_twice() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/same.css", ".a { color: red; }\n");

    let first = snapshot(&target, workspace, &read(&target)).expect("rev1");
    assert_eq!(first.rev, 1);
    // 同内容再压一次：复用栈顶，不多占一版。
    let again = snapshot(&target, workspace, &read(&target)).expect("dedup");
    assert_eq!(again.rev, 1);
    assert_eq!(revs_of(workspace, "temp/same.css"), vec![1]);
}

#[test]
fn legacy_history_without_cursor_steps_back_once() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/legacy.py", "C\n");
    fs::create_dir_all(workspace.join("temp/.history")).expect("mkdir");
    fs::write(workspace.join("temp/.history/legacy.py@1"), "A\n").expect("rev1");
    fs::write(workspace.join("temp/.history/legacy.py@2"), "B\n").expect("rev2");

    // 旧目录没有 @cursor：只允许退一步，之后就按「没有更多历史」处理。
    let back = step_back(&target, workspace, "C\n").expect("undo");
    assert_eq!(back.content, "B\n");
    assert_eq!(back.cursor, 0);
    fs::write(&target, &back.content).expect("write back");
    commit_cursor(&target, workspace, back.cursor).expect("cursor");
    assert_eq!(
        step_back(&target, workspace, "B\n").unwrap_err(),
        StepBlock::AtOldest { revisions: 2 }
    );
}

#[test]
fn tampered_snapshot_is_dropped_instead_of_restored() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/fake.py", "A\n");
    commit(&target, workspace, "A\n", "B\n");
    let (dir, name) = locate(&target, workspace).expect("locate");

    // 名字是对的（sha 取自真内容），里面的内容被换成假的。
    let path = dir.join(entry_name(&name, 1, "A\n"));
    assert_eq!(read_snapshot(&dir, &name, 1).expect("intact").content, "A\n");
    fs::write(&path, "fake-40\n").expect("forge");
    assert_eq!(
        read_snapshot(&dir, &name, 1).unwrap_err(),
        SnapshotError::Corrupt(path.clone())
    );
    // 读取只报损坏、不动盘；真正删除发生在撤销时（要连带压实编号与游标）。
    assert!(path.exists());
    assert!(matches!(
        step_back(&target, workspace, "B\n"),
        Err(StepBlock::Corrupt { .. })
    ));
    assert!(!path.exists(), "损坏的快照必须删掉，不能留着下次再恢复");
    assert_eq!(read(&target), "B\n", "用户文件不能被伪造内容覆盖");
    // 唯一的一版没了，历史确实空了。
    assert!(matches!(
        step_back(&target, workspace, "B\n"),
        Err(StepBlock::NoHistory)
    ));
}

#[test]
fn corrupt_middle_snapshot_does_not_hide_earlier_history() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/seq.css", "A\n");
    // A → B → C：@1=A、@2=B，游标 2，文件是 C。
    commit(&target, workspace, "A\n", "B\n");
    commit(&target, workspace, "B\n", "C\n");
    let (dir, name) = locate(&target, workspace).expect("locate");
    fs::write(dir.join(entry_name(&name, 2, "B\n")), "fake-40\n").expect("forge");

    // 第一次撤销撞上损坏的 @2：只报损坏，文件不动，编号与游标一起压实。
    assert!(matches!(
        step_back(&target, workspace, "C\n"),
        Err(StepBlock::Corrupt { .. })
    ));
    assert_eq!(read(&target), "C\n");
    assert_eq!(revs_of(workspace, "temp/seq.css"), vec![1]);
    assert_eq!(read_cursor(&dir, &name), Some(1));

    // 更早的 @1 不能被跳过：继续撤销要退到 A。
    let back = step_back(&target, workspace, "C\n").expect("undo to earliest");
    assert_eq!(back.content, "A\n");
    assert_eq!(back.cursor, 0);
    fs::write(&target, &back.content).expect("write back");
    commit_cursor(&target, workspace, back.cursor).expect("cursor");

    // 撤销过程中当前内容 C 被补成 redo 来源，不能丢。
    let forward = step_forward(&target, workspace).expect("redo");
    assert_eq!(forward.content, "C\n");
    assert_eq!(forward.cursor, 1);
}

#[test]
fn stale_cursor_after_a_manual_delete_falls_back_to_the_nearest_revision() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/gap2.py", "A\n");
    commit(&target, workspace, "A\n", "B\n");
    commit(&target, workspace, "B\n", "C\n");
    let (dir, name) = locate(&target, workspace).expect("locate");

    // 旧版本删中间格但不压实会留下这种状态：cursor=2，@2 已不在。
    remove_rev(&dir, &name, 2);
    assert_eq!(read_cursor(&dir, &name), Some(2));

    // 不能宣布「已到最早」：要退到游标之下最近还在的 @1。
    let back = step_back(&target, workspace, "C\n").expect("fallback to @1");
    assert_eq!(back.content, "A\n");
    assert_eq!(back.cursor, 0);
    // 当前内容被补成 redo 来源，编号重新连续。
    assert_eq!(revs_in(&dir, &name), vec![1, 2]);
}

#[test]
fn forged_high_revision_does_not_drag_the_numbering_along() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let target = seed(workspace, "temp/gap.py", "A\n");
    commit(&target, workspace, "A\n", "B\n");
    let (dir, name) = locate(&target, workspace).expect("locate");

    // 伪造一份 @41：写入只接着游标编号，并把游标之上的一切（含伪造件）清掉。
    let forged = dir.join(entry_name(&name, 41, "fake-40\n"));
    fs::write(&forged, "fake-40\n").expect("forge");
    let snap = snapshot(&target, workspace, "B\n").expect("snapshot");
    assert_eq!(snap.rev, 2, "新版本号只能接着游标走");
    assert_eq!(revs_of(workspace, "temp/gap.py"), vec![1, 2]);
    assert!(!forged.exists(), "游标之上的伪造版本要清掉");
}

#[test]
fn sweep_drops_versions_older_than_the_retention_window() {
    let root = tempfile::tempdir().expect("tempdir");
    let workspace = root.path();
    let history = history_root(workspace);
    fs::create_dir_all(&history).expect("mkdir");
    let old = history.join("stale.py@1");
    fs::write(&old, "x\n").expect("seed");
    // 把修改时间推到保留窗口之外。
    let stamp = SystemTime::now() - Duration::from_secs(MAX_AGE_SECS + 60);
    set_mtime(&old, stamp);

    let fresh = history.join("stale.py@2");
    fs::write(&fresh, "y\n").expect("seed");

    sweep(&history);
    // 过期的那格被删，剩下的编号压实：原来的 @2 变成 @1，内容不变。
    assert_eq!(
        fs::read_to_string(history.join("stale.py@1")).expect("survivor"),
        "y\n",
        "留下的版本要前移成 @1"
    );
    assert!(!history.join("stale.py@2").exists(), "压实后不该留空洞");
    assert!(!fresh.exists());
}

fn set_mtime(path: &Path, at: SystemTime) {
    let file = fs::File::options()
        .write(true)
        .open(path)
        .expect("open for mtime");
    file.set_modified(at).expect("set mtime");
}
