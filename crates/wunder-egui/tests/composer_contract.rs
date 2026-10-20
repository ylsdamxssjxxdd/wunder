//! 跨模块契约测试：`composer_attachments`、`thread_list`、`timeline_footer`、
//! `timeline_ruler` 被 `app.rs` 组合使用时的默认值、限额、排序与返回语义。
//!
//! 这四个模块各自已有单元测试；本文件只锁「app 依赖的组合假设」：限额之间
//! 的优先级、空输入时应当画出什么、集合划分是否覆盖且不重叠、以及同一个轮次
//! 在两个模块里是否给出同一个序号。全部走公开 API，不使用
//! `unwrap()/expect()/panic!()`。

use wunder_egui::composer_attachments::{
    AttachError, Attachment, Attachments, History, PromptQueue, QueueError, MAX_ATTACHMENTS,
    MAX_HISTORY, MAX_QUEUED, MAX_TOTAL_BYTES,
};
use wunder_egui::model::{MessageView, TurnView};
use wunder_egui::thread_list::{archivable, ThreadSelection};
use wunder_egui::timeline_footer::{estimated_tokens, facts_from_turn, segments, FooterFacts};
use wunder_egui::timeline_ruler::{
    hovered, summary_text, ticks, visible, HIT_TOLERANCE, MIN_TICK_HEIGHT, SUMMARY_MAX_CHARS,
};

fn file(name: &str, size: u64) -> Attachment {
    Attachment::new(format!("C:/tmp/{name}"), name, size)
}

fn dir(name: &str) -> Attachment {
    Attachment::directory(format!("C:/tmp/{name}"), name)
}

fn segment_texts(facts: &FooterFacts) -> Vec<String> {
    segments(facts)
        .into_iter()
        .map(|segment| segment.text)
        .collect()
}

#[test]
fn attachment_count_limit_reports_the_same_number_it_enforces() {
    // 锁：第 9 个附件被拒，且用户看到的提示数字必须等于 MAX_ATTACHMENTS。
    let mut set = Attachments::new();
    for index in 0..MAX_ATTACHMENTS {
        assert_eq!(set.add(file(&format!("f{index}.txt"), 1)), Ok(()));
    }
    assert_eq!(set.len(), MAX_ATTACHMENTS);

    assert_eq!(set.add(file("overflow.txt", 1)), Err(AttachError::TooMany));
    assert_eq!(set.len(), MAX_ATTACHMENTS);
    assert_eq!(set.total_bytes(), MAX_ATTACHMENTS as u64);

    let quoted = MAX_ATTACHMENTS.to_string();
    assert!(
        AttachError::TooMany.message().contains(&quoted),
        "message was {}",
        AttachError::TooMany.message()
    );
}

#[test]
fn attachment_byte_budget_is_inclusive_and_reported_in_the_error_text() {
    // 锁：字节上限是「含等号」的，超出 1 字节即拒，且提示里写的是同一个兆字节数。
    let mut set = Attachments::new();
    assert_eq!(set.add(file("full.bin", MAX_TOTAL_BYTES)), Ok(()));
    assert_eq!(set.total_bytes(), MAX_TOTAL_BYTES);

    assert_eq!(set.add(file("one-more.bin", 1)), Err(AttachError::TooLarge));
    assert_eq!(set.len(), 1);
    assert_eq!(set.total_bytes(), MAX_TOTAL_BYTES);

    let quoted_mib = (MAX_TOTAL_BYTES / (1024 * 1024)).to_string();
    assert!(
        AttachError::TooLarge.message().contains(&quoted_mib),
        "message was {}",
        AttachError::TooLarge.message()
    );

    // 单个超大文件同样被拒，且不会留下半个附件。
    let mut oversized = Attachments::new();
    assert_eq!(
        oversized.add(file("huge.bin", MAX_TOTAL_BYTES + 1)),
        Err(AttachError::TooLarge)
    );
    assert!(oversized.is_empty());
}

#[test]
fn directories_spend_a_slot_but_not_the_byte_budget() {
    // 锁：目录只占数量名额，不占字节额度；字节已满时目录仍可加入，但受数量上限约束。
    let mut set = Attachments::new();
    assert_eq!(set.add(file("full.bin", MAX_TOTAL_BYTES)), Ok(()));
    assert_eq!(set.add(dir("src")), Ok(()));
    assert_eq!(set.len(), 2);
    assert_eq!(set.total_bytes(), MAX_TOTAL_BYTES);

    for index in 2..MAX_ATTACHMENTS {
        assert_eq!(set.add(dir(&format!("d{index}"))), Ok(()));
    }
    assert_eq!(set.len(), MAX_ATTACHMENTS);
    assert_eq!(set.total_bytes(), MAX_TOTAL_BYTES);

    assert_eq!(set.add(dir("overflow")), Err(AttachError::TooMany));
    assert_eq!(set.len(), MAX_ATTACHMENTS);
}

#[test]
fn duplicate_path_precedes_the_count_limit_and_is_recoverable() {
    // 锁：重复 path 先于数量检查被判定（满额时也是 Duplicate），remove 之后可以重新加入。
    let mut set = Attachments::new();
    assert_eq!(set.add(file("a.txt", 10)), Ok(()));

    // 同一个 path，不同的名字和大小：仍算重复，且不覆盖第一次的信息。
    let renamed = Attachment::new("C:/tmp/a.txt", "renamed.txt", 999);
    assert_eq!(set.add(renamed), Err(AttachError::Duplicate));
    assert_eq!(set.len(), 1);
    assert_eq!(set.total_bytes(), 10);
    assert_eq!(
        set.items().first().map(|item| item.name.as_str()),
        Some("a.txt")
    );

    assert!(set.remove("C:/tmp/a.txt"));
    assert_eq!(set.add(file("a.txt", 20)), Ok(()));
    assert_eq!(set.len(), 1);
    assert_eq!(set.total_bytes(), 20);

    // 满额时重复的 path 仍然是 Duplicate，而不是 TooMany：顺序是文档化的契约。
    let mut full = Attachments::new();
    for index in 0..MAX_ATTACHMENTS {
        assert_eq!(full.add(file(&format!("f{index}.txt"), 1)), Ok(()));
    }
    assert_eq!(full.add(file("f0.txt", 1)), Err(AttachError::Duplicate));
    assert_eq!(full.len(), MAX_ATTACHMENTS);
}

#[test]
fn chips_mirror_items_in_order_and_the_summary_count_agrees() {
    // 锁：chips 与 items 一一对应且顺序相同，summary 的数量前缀等于实际条数。
    let mut set = Attachments::new();
    assert_eq!(set.add(file("a.txt", 12 * 1024)), Ok(()));
    assert_eq!(set.add(dir("src")), Ok(()));
    assert_eq!(set.add(file("b.bin", 1024 * 1024 + 200 * 1024)), Ok(()));

    let chips = set.chips();
    assert_eq!(chips.len(), set.len());
    assert_eq!(set.len(), set.items().len());

    let chip_paths: Vec<&str> = chips.iter().map(|chip| chip.path.as_str()).collect();
    let item_paths: Vec<&str> = set.items().iter().map(|item| item.path.as_str()).collect();
    assert_eq!(chip_paths, item_paths);
    assert!(chips.iter().all(|chip| chip.removable));

    let summary = set.summary();
    assert!(
        summary.starts_with(&format!("{} 个附件", set.len())),
        "summary was {summary}"
    );

    // 空集是「不画」而不是「0 个附件」：两边的空态必须一致。
    set.clear();
    assert_eq!(set.summary(), "");
    assert!(set.chips().is_empty());
}

#[test]
fn queue_is_fifo_and_stops_at_the_queue_limit() {
    // 锁：队列满 4 条后拒收（提示数字与 MAX_QUEUED 一致），弹出顺序严格等于入队顺序。
    let mut queue = PromptQueue::new();
    for index in 0..MAX_QUEUED {
        assert_eq!(queue.push(format!("p{index}")), Ok(()));
    }
    assert_eq!(queue.len(), MAX_QUEUED);
    assert_eq!(queue.push("p4".to_owned()), Err(QueueError::Full));
    assert_eq!(queue.len(), MAX_QUEUED);
    assert!(
        QueueError::Full.message().contains(&MAX_QUEUED.to_string()),
        "message was {}",
        QueueError::Full.message()
    );

    assert_eq!(queue.pop(), Some("p0".to_owned()));
    assert_eq!(queue.pop(), Some("p1".to_owned()));
    assert_eq!(
        queue.items(),
        ["p2".to_owned(), "p3".to_owned()],
        "剩余队列必须保持插入顺序"
    );
    // 让出名额后可以继续入队。
    assert_eq!(queue.pop(), Some("p2".to_owned()));
    assert_eq!(queue.push("p4".to_owned()), Ok(()));
    assert_eq!(queue.pop(), Some("p3".to_owned()));
    assert_eq!(queue.pop(), Some("p4".to_owned()));
    assert_eq!(queue.pop(), None);
    assert!(queue.is_empty());
}

#[test]
fn queue_remove_keeps_the_remaining_rows_in_order() {
    // 锁：remove(i) 只删一行，items() 里其余条目的相对顺序不变，越界索引是 false。
    let mut queue = PromptQueue::new();
    assert_eq!(queue.push("a".to_owned()), Ok(()));
    assert_eq!(queue.push("b".to_owned()), Ok(()));
    assert_eq!(queue.push("c".to_owned()), Ok(()));

    assert!(queue.remove(1));
    assert_eq!(queue.items(), ["a".to_owned(), "c".to_owned()]);
    assert_eq!(queue.len(), 2);
    // 越界不 panic，也不改变内容。
    assert!(!queue.remove(2));
    assert!(!queue.remove(usize::MAX));
    assert_eq!(queue.items(), ["a".to_owned(), "c".to_owned()]);

    assert_eq!(queue.pop(), Some("a".to_owned()));
    assert_eq!(queue.items(), ["c".to_owned()]);
}

#[test]
fn history_walks_to_the_oldest_entry_and_back_to_the_draft() {
    // 锁：连续 previous 依次给最新到最旧并在最旧处停住，next 回到最新后返回 None 且草稿保留。
    let mut history = History::new();
    history.push("one");
    history.push("two");
    history.push("three");

    assert_eq!(history.previous("draft"), Some("three".to_owned()));
    assert_eq!(history.previous("three"), Some("two".to_owned()));
    assert_eq!(history.previous("two"), Some("one".to_owned()));
    // 最旧一条处不越界：重复返回同一条。
    assert_eq!(history.previous("one"), Some("one".to_owned()));
    assert_eq!(history.draft(), "draft");

    assert_eq!(history.next(), Some("two".to_owned()));
    assert_eq!(history.next(), Some("three".to_owned()));
    assert_eq!(history.next(), None);
    assert_eq!(history.draft(), "draft", "越过最新条后必须先还回原草稿");
    // 已经在草稿上：再按 down 仍然是 None。
    assert_eq!(history.next(), None);
    assert_eq!(history.draft(), "draft");
}

#[test]
fn history_keeps_the_newest_max_history_entries_after_trimming() {
    // 锁：历史超过 MAX_HISTORY 条后修剪最旧的，回头最多只能走 MAX_HISTORY 步，且最新条始终可达。
    let mut history = History::new();
    for index in 0..(MAX_HISTORY + 10) {
        history.push(&format!("p{index}"));
    }

    let mut seen: Vec<String> = Vec::new();
    let mut current = "draft".to_owned();
    for _ in 0..(MAX_HISTORY * 2) {
        let Some(entry) = history.previous(&current) else {
            break;
        };
        if seen.last().map(String::as_str) == Some(entry.as_str()) {
            // 已经在最旧一条上，`previous` 会原地返回。
            break;
        }
        current = entry.clone();
        seen.push(entry);
    }

    assert_eq!(seen.len(), MAX_HISTORY, "历史长度必须被修剪到上限");
    assert_eq!(
        seen.first().map(String::as_str),
        Some("p59"),
        "最新一次发送必须还能被取到"
    );

    // 重置游标后，最新条仍然可达（说明修剪没有动到尾部）。
    history.reset();
    assert_eq!(history.previous("fresh"), Some("p59".to_owned()));
    assert_eq!(history.draft(), "fresh");
}

#[test]
fn selection_activate_then_toggle_and_exit_leave_no_residue() {
    // 锁：activate 进入多选并选中，toggle 只改集合不改模式，exit 同时清空集合与模式。
    let mut selection = ThreadSelection::new();
    assert!(!selection.is_active());

    selection.activate("a");
    assert!(selection.is_active());
    selection.toggle("b");
    assert_eq!(selection.len(), 2);
    assert!(selection.contains("a") && selection.contains("b"));

    selection.exit();
    assert!(!selection.is_active());
    assert_eq!(selection.len(), 0);
    assert!(selection.ids().is_empty());

    // 重新进入时旧 id 不得复活。
    selection.activate("c");
    assert_eq!(selection.ids(), vec!["c"]);
}

#[test]
fn select_all_deduplicates_ids_without_duplicating_the_count() {
    // 锁：select_all 传入含重复 id 的迭代器后，计数等于去重后的个数，且模式不变。
    let mut selection = ThreadSelection::new();
    selection.enter();
    selection.select_all(["b", "a", "b"]);

    assert_eq!(selection.len(), 2);
    assert_eq!(selection.ids(), vec!["a", "b"]);
    assert!(selection.is_active(), "select_all 不负责退出多选");
}

#[test]
fn archive_partition_is_a_disjoint_ordered_cover_of_the_selection() {
    // 锁：archive_targets 与 skipped 按传入顺序划分选中集合，并集等于选中集合、交集为空。
    let rows = [("a", "done"), ("b", "running"), ("c", "done")];
    let mut selection = ThreadSelection::new();
    for (id, _) in rows {
        selection.activate(id);
    }

    assert!(archivable("done"));
    assert!(!archivable("running"), "运行中的线程不能批量归档");

    let targets = selection.archive_targets(rows.iter().copied());
    let skipped = selection.skipped(rows.iter().copied());

    assert_eq!(targets, vec!["a", "c"], "可归档项保持可见列表顺序");
    assert_eq!(skipped, vec!["b"], "被跳过的项同样保持列表顺序");
    assert!(
        targets
            .iter()
            .all(|id| !skipped.iter().any(|other| other == id)),
        "两个结果集不能有交集"
    );

    let mut union: Vec<&str> = targets
        .iter()
        .map(String::as_str)
        .chain(skipped.iter().map(String::as_str))
        .collect();
    union.sort_unstable();
    assert_eq!(union, vec!["a", "b", "c"], "并集必须覆盖全部选中项");

    let mut selected = selection.ids();
    selected.sort_unstable();
    assert_eq!(selected, union);
}

#[test]
fn footer_status_chip_survives_missing_facts_but_default_facts_paint_nothing() {
    // 锁：只有状态可用时 footer 仍给出一个状态段（其它缺失字段不画占位符）；
    // 而所有字段都取默认值（state 为空、round 为 0）时 segments 为空。
    let only_status = FooterFacts {
        state: "running".to_owned(),
        ..Default::default()
    };
    assert_eq!(segment_texts(&only_status), vec!["运行中".to_owned()]);

    let empty = FooterFacts::default();
    assert_eq!(empty.round, 0);
    assert!(
        segments(&empty).is_empty(),
        "默认 facts 没有状态也没有轮次，app 必须能接受空段列表"
    );
}

#[test]
fn footer_speed_chip_needs_both_duration_and_tokens() {
    // 锁：耗时与 token 同时存在才出现 tok/s 段；缺任一端时既不画速度也不画 0 tok/s。
    let both = FooterFacts {
        elapsed_ms: Some(10_000),
        output_tokens: Some(420),
        ..Default::default()
    };
    let speed = segments(&both)
        .into_iter()
        .find(|segment| segment.text.contains("tok/s"));
    assert_eq!(
        speed.as_ref().map(|segment| segment.text.as_str()),
        Some("42 tok/s")
    );
    assert!(speed
        .map(|segment| segment.tooltip)
        .unwrap_or_default()
        .contains("输出速度"));

    let tokens_only = FooterFacts {
        output_tokens: Some(420),
        ..Default::default()
    };
    assert!(segments(&tokens_only)
        .iter()
        .all(|segment| !segment.text.contains("tok/s")));

    // 只有耗时时保留耗时段，速度段缺席。
    let elapsed_only = FooterFacts {
        elapsed_ms: Some(1_200),
        ..Default::default()
    };
    assert_eq!(segment_texts(&elapsed_only), vec!["1.2s".to_owned()]);
}

#[test]
fn footer_zero_context_capacity_stays_finite() {
    // 锁：上下文上限为 0 时退化成「只显示已用」，不除零、不出现 inf/NaN 文本。
    let facts = FooterFacts {
        context_tokens: Some(12_800),
        context_capacity_tokens: Some(0),
        ..Default::default()
    };
    let chips = segments(&facts);
    assert_eq!(chips.len(), 1);
    assert_eq!(chips.first().map(|chip| chip.text.as_str()), Some("12.8K"));

    for chip in &chips {
        let joined = format!("{}{}", chip.text, chip.tooltip);
        assert!(
            !joined.contains("inf") && !joined.contains("NaN"),
            "non-finite text: {joined}"
        );
    }
}

#[test]
fn footer_round_and_ruler_ordinals_are_both_one_based() {
    // 锁：同一个零基轮次索引在 footer 与 ruler 里得到的都是「第 N+1 轮」，不会差一位。
    let turn = TurnView {
        assistant: MessageView {
            state: "completed".to_owned(),
            text: "你好世界".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    };
    let facts = facts_from_turn(&turn, 2, None);

    assert_eq!(facts.round, 3);
    assert_eq!(estimated_tokens("你好世界"), 4);
    assert_eq!(facts.output_tokens, Some(estimated_tokens("你好世界")));
    assert!(facts.tokens_estimated, "估算来的 token 必须被标记");

    assert!(summary_text(2, "帮我整理这份资料").starts_with("第 3 轮"));
    let round_text = segments(&facts)
        .into_iter()
        .find(|segment| segment.text.contains("轮"))
        .map(|segment| segment.text)
        .unwrap_or_default();
    assert_eq!(round_text, "第 3 轮");
}

#[test]
fn ruler_hides_below_two_turns_while_ticks_need_the_visible_gate() {
    // 锁：0/1 轮时 visible 为 false（app 必须先据此跳过整条 lane）；ticks 自身不判轮数，
    // 单轮仍会投影 1 个 band，所以「不画」只能由 visible 保证。
    assert!(!visible(0));
    assert!(!visible(1));
    assert!(visible(2));

    assert!(ticks(0, 1_000.0, 0.0, 600.0, 0.0, 480.0).is_empty());
    let single = ticks(1, 1_000.0, 0.0, 600.0, 0.0, 480.0);
    assert_eq!(single.len(), 1);
    assert!(!visible(single.len()), "单轮 ticks 存在但 lane 仍不可见");

    let strip = ticks(5, 1_000.0, 0.0, 600.0, 0.0, 480.0);
    assert!(visible(5));
    assert_eq!(strip.len(), 5);
    for pair in strip.windows(2) {
        assert!(pair[1].y >= pair[0].y, "tick 的 y 必须单调不减");
    }
    for (index, tick) in strip.iter().enumerate() {
        assert_eq!(tick.turn_index, index);
        assert!(
            (0.0..=480.0).contains(&tick.y),
            "tick y={} 超出 [0, ruler_height]",
            tick.y
        );
        assert!(tick.height >= MIN_TICK_HEIGHT);
    }

    // 点击/悬停仍按 tick 的 turn_index 回报，和上面的顺序一致。
    assert_eq!(hovered(&strip, 10.0, HIT_TOLERANCE), Some(0));
}

#[test]
fn ruler_summary_truncates_by_chars_not_bytes() {
    // 锁：首行超过 SUMMARY_MAX_CHARS 个字符时按字符截断并补省略号，中文不会被切坏。
    let long = "汉".repeat(SUMMARY_MAX_CHARS + 1);
    let summary = summary_text(0, &long);
    let expected = format!("第 1 轮 · {}…", "汉".repeat(SUMMARY_MAX_CHARS));
    assert_eq!(summary, expected);
    assert_eq!(summary.matches('汉').count(), SUMMARY_MAX_CHARS);
    assert_eq!(
        summary.chars().count(),
        "第 1 轮 · ".chars().count() + SUMMARY_MAX_CHARS + 1
    );

    // 正好达到预算时不该出现省略号。
    let exact = "a".repeat(SUMMARY_MAX_CHARS);
    assert!(!summary_text(0, &exact).contains('…'));

    // 空首行退化成纯序号，仍与 footer 的 1-based 序号一致。
    assert_eq!(summary_text(4, "   "), "第 5 轮");
    assert_eq!(summary_text(4, "第一行\n第二行"), "第 5 轮 · 第一行");
}
