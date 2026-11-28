use super::*;

fn entry(value: f64) -> Entry {
    Entry {
        value,
        tokens: value as i64,
        kind: TileKind::Item,
        slot: 1,
        tag: String::new(),
        name: String::new(),
        count: 0,
        item: 0,
        its: Vec::new(),
    }
}

#[test]
fn squarify_covers_the_rect_without_overlap() {
    let entries = vec![entry(3000.0), entry(1000.0), entry(500.0), entry(200.0)];
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 800.0,
        h: 200.0,
    };
    let laid = squarify(&entries, rect);
    assert_eq!(laid.len(), entries.len());
    for (a, _) in &laid {
        assert!(a.x >= -0.001 && a.y >= -0.001);
        assert!(a.x + a.w <= 800.001 && a.y + a.h <= 200.001);
    }
    for (index, (a, _)) in laid.iter().enumerate() {
        for (b, _) in laid.iter().skip(index + 1) {
            let overlap_x = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
            let overlap_y = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
            assert!(overlap_x <= 0.001 || overlap_y <= 0.001);
        }
    }
}

#[test]
fn tile_labels_are_bare_when_the_tile_cannot_hold_text() {
    let record = Record {
        window: 100_000,
        tokens: 20_000,
        parts: vec![PartRecord {
            slot: 2,
            tokens: 20_000,
            items: vec![],
        }],
        ..Default::default()
    };
    let tiles = layout(&record, 880.0, 232.0, true).tiles;
    assert!(!tiles.is_empty());
    assert!(tiles.iter().all(|tile| tile.w > 0.0 && tile.h > 0.0));
    assert!(tiles.iter().any(|tile| tile.kind == TileKind::Free));
}

#[test]
fn tiles_stay_inside_the_map_box() {
    let record = Record {
        window: 128_000,
        tokens: 58_200,
        parts: vec![
            PartRecord {
                slot: 1,
                tokens: 14_500,
                items: vec![ItemRecord {
                    tag: "prompt".into(),
                    name: String::new(),
                    count: 1,
                    tokens: 14_500,
                }],
            },
            PartRecord {
                slot: 2,
                tokens: 15_700,
                items: vec![
                    ItemRecord {
                        tag: "tool".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 6_200,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "bash".into(),
                        count: 1,
                        tokens: 3_100,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "web_search".into(),
                        count: 1,
                        tokens: 2_600,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "write_file".into(),
                        count: 1,
                        tokens: 1_800,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "list_dir".into(),
                        count: 1,
                        tokens: 1_400,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "util1".into(),
                        count: 1,
                        tokens: 200,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "util2".into(),
                        count: 1,
                        tokens: 200,
                    },
                    ItemRecord {
                        tag: "tool".into(),
                        name: "util3".into(),
                        count: 1,
                        tokens: 200,
                    },
                ],
            },
            PartRecord {
                slot: 3,
                tokens: 1_500,
                items: vec![ItemRecord {
                    tag: "image".into(),
                    name: String::new(),
                    count: 1,
                    tokens: 1_500,
                }],
            },
            PartRecord {
                slot: 4,
                tokens: 4_000,
                items: vec![
                    ItemRecord {
                        tag: "result".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 1_600,
                    },
                    ItemRecord {
                        tag: "result".into(),
                        name: "read_file".into(),
                        count: 1,
                        tokens: 800,
                    },
                    ItemRecord {
                        tag: "result".into(),
                        name: "web_search".into(),
                        count: 1,
                        tokens: 1_000,
                    },
                    ItemRecord {
                        tag: "result".into(),
                        name: "list_dir".into(),
                        count: 1,
                        tokens: 600,
                    },
                ],
            },
            PartRecord {
                slot: 5,
                tokens: 14_200,
                items: vec![
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 1,
                        tokens: 4_200,
                    },
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 2,
                        tokens: 3_600,
                    },
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 3,
                        tokens: 2_800,
                    },
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 4,
                        tokens: 1_800,
                    },
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 5,
                        tokens: 1_000,
                    },
                    ItemRecord {
                        tag: "turn".into(),
                        name: String::new(),
                        count: 6,
                        tokens: 800,
                    },
                ],
            },
        ],
        ..Default::default()
    };
    for (w, h) in [(880.0, 548.0), (416.0, 548.0), (432.0, 240.0)] {
        let laid = layout(&record, w, h, true);
        for tile in &laid.tiles {
            assert!(tile.x >= -0.001, "x={}", tile.x);
            assert!(tile.y >= -0.001, "y={}", tile.y);
            assert!(tile.x + tile.w <= 1.001, "x+w={}", tile.x + tile.w);
            assert!(tile.y + tile.h <= 1.001, "y+h={}", tile.y + tile.h);
            assert!(tile.w > 0.0 && tile.h > 0.0);
        }
        // The free remainder stays the last tile laid.
        let free = laid.tiles.last().expect("last tile");
        assert_eq!(free.kind, TileKind::Free);
    }
}

#[test]
fn token_counts_read_like_the_reference() {
    assert_eq!(fmt_k(8300), "8.3K");
    assert_eq!(fmt_k(992_000), "992K");
    assert_eq!(fmt_k(120_000), "120K");
    assert_eq!(fmt_k(1_000_000), "1M");
    assert_eq!(fmt_k(1_250_000), "1.25M");
    assert_eq!(fmt_k(0), "0");
}

#[test]
fn a_nearly_empty_window_keeps_the_system_part_readable() {
    let record = Record {
        window: 1_000_000,
        tokens: 8_300,
        parts: vec![
            PartRecord {
                slot: 1,
                tokens: 3_100,
                items: vec![ItemRecord {
                    tag: "prompt".into(),
                    name: String::new(),
                    count: 1,
                    tokens: 3_100,
                }],
            },
            PartRecord {
                slot: 2,
                tokens: 4_400,
                items: [
                    507, 466, 417, 322, 281, 260, 240, 220, 200, 180, 170, 160, 150, 140, 130, 120,
                    110, 100, 227,
                ]
                .iter()
                .enumerate()
                .map(|(i, tokens)| ItemRecord {
                    tag: "tool".into(),
                    name: format!("tool{i}"),
                    count: 1,
                    tokens: *tokens,
                })
                .collect(),
            },
            PartRecord {
                slot: 5,
                tokens: 748,
                items: vec![ItemRecord {
                    tag: "turn".into(),
                    name: String::new(),
                    count: 1,
                    tokens: 748,
                }],
            },
        ],
        ..Default::default()
    };
    let laid = layout(&record, 880.0, 232.0, true);
    let system = laid
        .tiles
        .iter()
        .find(|tile| tile.slot == 1)
        .expect("system tile");
    assert!(system.w * 880.0 > 100.0, "system tile width {}", system.w);
    assert!(system.h * 232.0 > 40.0, "system tile height {}", system.h);
    assert!(laid.tiles.iter().any(|tile| tile.kind == TileKind::Free));
}

#[test]
fn portrait_and_landscape_keep_drillable_tiles_for_the_same_snapshot() {
    let make_item = |tokens| ItemRecord {
        tag: "tool".into(),
        tokens,
        ..Default::default()
    };
    let record = Record {
        window: 128_000,
        tokens: 12_000,
        parts: vec![
            PartRecord {
                slot: 1,
                tokens: 3_000,
                items: vec![],
            },
            PartRecord {
                slot: 2,
                tokens: 9_000,
                items: vec![
                    make_item(7_800),
                    make_item(1100),
                    make_item(60),
                    make_item(40),
                ],
            },
            PartRecord {
                slot: 3,
                tokens: 0,
                items: vec![],
            },
        ],
        ..Default::default()
    };
    for (width, height) in [(296.0, 520.0), (366.0, 650.0), (776.0, 190.0)] {
        let mobile = layout(&record, width, height, false);
        let desktop = layout(&record, width, height, true);
        assert!(mobile.tips.is_empty());
        assert_eq!(desktop.tips.len(), mobile.tiles.len());
        assert!(mobile.tiles.iter().any(|tile| tile.kind == TileKind::Item));
        assert!(mobile.tiles.iter().any(|tile| tile.kind == TileKind::More));
        assert!(mobile.tiles.iter().all(|tile| tile.slot != 3));
        assert_eq!(
            mobile.tiles.iter().map(|tile| tile.tokens).sum::<i64>(),
            record.window
        );
        for (tile, reference) in mobile.tiles.iter().zip(&desktop.tiles) {
            assert_eq!(
                [tile.x, tile.y, tile.w, tile.h],
                [reference.x, reference.y, reference.w, reference.h]
            );
            assert!(tile.x >= 0.0 && tile.y >= 0.0 && tile.w > 0.0 && tile.h > 0.0);
            assert!(tile.x + tile.w <= 1.000_001 && tile.y + tile.h <= 1.000_001);
        }
        let free = mobile.tiles.last().unwrap();
        assert_eq!(free.kind, TileKind::Free);
        assert_eq!(free.display_value(&record), "116K · 91%");
        assert_eq!(free.x == 0.0, height > width);
    }
}

#[test]
fn only_a_known_positive_remainder_gets_a_free_tile() {
    let mut record = Record {
        window: 128_000,
        ..Default::default()
    };
    assert!(layout(&record, 296.0, 520.0, false).tiles.is_empty());
    record.tokens = 128_000;
    record.parts.push(PartRecord {
        slot: 1,
        tokens: record.tokens,
        items: vec![],
    });
    for window in [0, 128_000, 64_000] {
        record.window = window;
        let layout = layout(&record, 296.0, 520.0, false);
        assert_eq!(layout.tiles.len(), 1);
        assert_eq!(layout.tiles[0].kind, TileKind::Part);
        assert_eq!([layout.tiles[0].w, layout.tiles[0].h], [1.0, 1.0]);
    }
}
