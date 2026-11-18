// AI生成
/**
 * Place the live print of the model round that is still streaming.
 *
 * The work process already holds a durable row for that round (an
 * `assistant_message` item with tool calls keeps its print inside the fold). A
 * second, appended "live" block printed the same prose twice — once in place,
 * once at the bottom of the fold — and the fold then also leaked that prose into
 * the transcript. The live text therefore never adds a row: it is printed into
 * the row that owns the same item, and only when that row is not on the loaded
 * page does the caller fall back to one standalone block.
 */
export interface LivePrintRow {
  kind: string;
  key: string;
  text?: string;
  live?: boolean;
}

export interface LivePrintPlacement<T> {
  rows: T[];
  /** True when the live text was printed into an existing row. */
  placed: boolean;
}

export function placeLivePrint<T extends LivePrintRow>(
  rows: T[],
  live: string,
  liveKey: string,
  /** False while the reader is browsing history: never print live text there. */
  newest: boolean
): LivePrintPlacement<T> {
  if (!live || !liveKey || !newest) return { rows, placed: false };
  let placed = false;
  const next = rows.map((row) => {
    if (row.kind !== 'print' || row.key !== liveKey) return row;
    placed = true;
    return { ...row, text: live, live: true } as T;
  });
  // Copying nothing would allocate a new array per frame for every turn that has
  // no live print; the caller keeps the previous array in that case.
  return placed ? { rows: next, placed } : { rows, placed: false };
}
