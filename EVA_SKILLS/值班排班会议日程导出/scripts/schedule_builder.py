#!/usr/bin/env python3
# -*- coding: utf-8 -*-
import argparse
import csv
import json
import uuid
from datetime import datetime
from pathlib import Path
from typing import Any, Dict, List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build shift/meeting schedules and export CSV/Markdown/ICS.",
    )
    parser.add_argument("--input", required=True, help="Input schedule JSON file.")
    parser.add_argument("--output-dir", required=True, help="Output directory.")
    return parser.parse_args()


def format_dt(date_text: str, time_text: str) -> str:
    return f"{date_text.replace('-', '')}T{time_text.replace(':', '')}00"


def ensure_list(value: Any) -> List[str]:
    if value is None:
        return []
    if isinstance(value, list):
        return [str(v) for v in value]
    return [str(value)]


def build_events(data: Dict[str, Any]) -> List[Dict[str, str]]:
    events: List[Dict[str, str]] = []
    for shift in data.get("shifts", []):
        person = shift.get("person", "")
        role = shift.get("role", "值班")
        title = f"{role} - {person}" if person else role
        events.append(
            {
                "type": "shift",
                "date": shift.get("date", ""),
                "start": shift.get("start", ""),
                "end": shift.get("end", ""),
                "title": title,
                "person": person,
                "organizer": "",
                "location": shift.get("location", ""),
                "notes": shift.get("notes", ""),
                "attendees": ", ".join(ensure_list(shift.get("attendees"))),
            }
        )
    for meeting in data.get("meetings", []):
        attendees = ensure_list(meeting.get("attendees"))
        events.append(
            {
                "type": "meeting",
                "date": meeting.get("date", ""),
                "start": meeting.get("start", ""),
                "end": meeting.get("end", ""),
                "title": meeting.get("title", "会议"),
                "person": "",
                "organizer": meeting.get("organizer", ""),
                "location": meeting.get("location", ""),
                "notes": meeting.get("notes", ""),
                "attendees": ", ".join(attendees),
            }
        )
    return events


def write_csv(events: List[Dict[str, str]], path: Path) -> None:
    fields = [
        "type",
        "date",
        "start",
        "end",
        "title",
        "person",
        "organizer",
        "location",
        "notes",
        "attendees",
    ]
    with path.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(events)


def render_table(headers: List[str], rows: List[List[str]]) -> List[str]:
    table = [
        "| " + " | ".join(headers) + " |",
        "| " + " | ".join(["---"] * len(headers)) + " |",
    ]
    for row in rows:
        table.append("| " + " | ".join(row) + " |")
    return table


def write_markdown(data: Dict[str, Any], events: List[Dict[str, str]], path: Path) -> None:
    lines: List[str] = [f"# {data.get('title', '日程表')}", ""]
    if data.get("range"):
        lines.append(
            f"- 时间范围：{data['range'].get('start', '')} ~ {data['range'].get('end', '')}"
        )
    lines.append(f"- 时区：{data.get('timezone', 'Asia/Shanghai')}")
    lines.append("")

    shift_rows = [
        [e["date"], e["start"], e["end"], e["title"], e["location"], e["notes"]]
        for e in events
        if e["type"] == "shift"
    ]
    meeting_rows = [
        [
            e["date"],
            e["start"],
            e["end"],
            e["title"],
            e["organizer"],
            e["location"],
            e["attendees"],
        ]
        for e in events
        if e["type"] == "meeting"
    ]

    lines.append("## 值班/排班")
    if shift_rows:
        lines.extend(render_table(["日期", "开始", "结束", "班次", "地点", "备注"], shift_rows))
    else:
        lines.append("（暂无）")
    lines.append("")

    lines.append("## 会议日程")
    if meeting_rows:
        lines.extend(render_table(["日期", "开始", "结束", "会议", "组织者", "地点", "参会"], meeting_rows))
    else:
        lines.append("（暂无）")
    lines.append("")

    path.write_text("\n".join(lines).strip() + "\n", encoding="utf-8")


def build_ics(events: List[Dict[str, str]], timezone: str) -> str:
    now = datetime.utcnow().strftime("%Y%m%dT%H%M%SZ")
    lines = [
        "BEGIN:VCALENDAR",
        "VERSION:2.0",
        "PRODID:-//wunder//schedule-builder//CN",
        "CALSCALE:GREGORIAN",
        "BEGIN:VTIMEZONE",
        f"TZID:{timezone}",
        "BEGIN:STANDARD",
        "DTSTART:19700101T000000",
        "TZOFFSETFROM:+0800",
        "TZOFFSETTO:+0800",
        "TZNAME:CST",
        "END:STANDARD",
        "END:VTIMEZONE",
    ]
    for event in events:
        uid = f"{uuid.uuid4()}@wunder"
        summary = event["title"]
        description_parts = []
        if event.get("notes"):
            description_parts.append(event["notes"])
        if event.get("attendees"):
            description_parts.append(f"参会：{event['attendees']}")
        description = "\\n".join(description_parts)
        lines.extend(
            [
                "BEGIN:VEVENT",
                f"UID:{uid}",
                f"DTSTAMP:{now}",
                f"DTSTART;TZID={timezone}:{format_dt(event['date'], event['start'])}",
                f"DTEND;TZID={timezone}:{format_dt(event['date'], event['end'])}",
                f"SUMMARY:{summary}",
            ]
        )
        if event.get("location"):
            lines.append(f"LOCATION:{event['location']}")
        if description:
            lines.append(f"DESCRIPTION:{description}")
        lines.append("END:VEVENT")
    lines.append("END:VCALENDAR")
    return "\r\n".join(lines) + "\r\n"


def main() -> int:
    args = parse_args()
    input_path = Path(args.input)
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    data = json.loads(input_path.read_text(encoding="utf-8-sig"))
    events = build_events(data)

    write_csv(events, output_dir / "schedule.csv")
    write_markdown(data, events, output_dir / "schedule_summary.md")
    ics_text = build_ics(events, data.get("timezone", "Asia/Shanghai"))
    (output_dir / "schedule.ics").write_text(ics_text, encoding="utf-8")

    print(f"Saved: {output_dir / 'schedule.csv'}")
    print(f"Saved: {output_dir / 'schedule_summary.md'}")
    print(f"Saved: {output_dir / 'schedule.ics'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
