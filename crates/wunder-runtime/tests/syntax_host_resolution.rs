//! 写后语法校验的宿主探测：改路径要立刻生效，不能等重启。
//!
//! 桌面版设置页有两件事依赖这条链路：
//! 1. 页面上显示的 node 路径必须是「此刻真的会被拿来校验的那个」，所以界面读
//!    [`syntax_hosts`]，和写入回执同源；
//! 2. 用户把路径从 A 改成 B 后，缓存里的旧结论必须作废，否则回执照旧用 A。
//!
//! 探测缓存和 `WUNDER_NODE` 都是进程级状态，所以这里只写一个测试：拆成多个并行
//! 用例会互相踩同一份状态。
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use wunder_server::{reset_syntax_hosts, syntax_hosts};

#[test]
fn syntax_hosts_adopt_a_new_node_override_without_a_restart() {
    let root = std::env::temp_dir().join(format!("wunder-syntax-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture dir");
    let stub = root.join("node");
    std::fs::write(&stub, "#!/bin/sh\nexit 0\n").expect("write node stub");
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
        .expect("make node stub executable");

    // 先探一次，把当前结论写进缓存。
    let before = syntax_hosts();
    // shell 标签只有三种可能：bash -n / sh -n / 没有 shell。
    assert!(
        matches!(before.shell_label.as_str(), "bash -n" | "sh -n" | ""),
        "unexpected shell label {:?}",
        before.shell_label
    );
    assert_eq!(
        before.shell.is_some(),
        !before.shell_label.is_empty(),
        "shell path and label must agree"
    );

    std::env::set_var("WUNDER_NODE", &stub);
    reset_syntax_hosts();
    let adopted = syntax_hosts();
    assert_eq!(
        adopted.node.as_deref(),
        Some(stub.to_string_lossy().as_ref()),
        "the override has to be adopted after the cache is dropped"
    );

    // 清掉覆盖并再次作废：探测结论必须重算，而不是继续复用桩程序。
    std::env::remove_var("WUNDER_NODE");
    reset_syntax_hosts();
    let after = syntax_hosts();
    assert_ne!(
        after.node.as_deref(),
        Some(stub.to_string_lossy().as_ref()),
        "dropping the override must drop the cached conclusion too"
    );
    assert_eq!(after.node, before.node);

    let _ = std::fs::remove_dir_all(&root);
}
