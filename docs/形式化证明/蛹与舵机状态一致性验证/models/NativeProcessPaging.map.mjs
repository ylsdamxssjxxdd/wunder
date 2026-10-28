// Chart labels for ProcessPaging, in the words of a reader who opens a long
// work process and wants the steps before the ones on screen.

export const readerActions = ["Open", "PressButton", "ScrollTop"];

const PANEL = { folded: "过程收着", open: "过程展开" };
const PAGE = {
	none: "还没读内容",
	latest: "显示最新一页",
	older: "显示更早一页",
};
const ENTRY = {
	button: "只能点按钮",
	scroll: "向上滑就能加载",
};
const EDGE = {
	unknown: "这一页没有续读位置",
	set: "这一页记着续读位置",
};

// Only what the reader can see: the fold, the page on screen, and whether more
// steps exist. The cursor and the entry are hidden state, so both models can be
// compared by these words.
export function state(v) {
	const left = v.older_left ? "还有更早的步骤" : "没有更早的步骤";
	return `${PANEL[v.panel]}，${PAGE[v.page]}，${left}`;
}

export function detail(v) {
	return `${ENTRY[v.entry]}，${EDGE[v.edge]}`;
}

export function group(v) {
	const stuck =
		v.panel === "open" &&
		v.older_left &&
		v.page === "latest" &&
		v.edge === "unknown";
	return stuck ? "有更早的步骤却取不到" : "能取到更早的步骤";
}

export const groupColors = {
	"能取到更早的步骤": "#e8f4ea",
	"有更早的步骤却取不到": "#fde8e8",
};

const STEPS = {
	Open: "展开工作过程",
	ReadLatest: "读到最新一页",
	Rebuild: "视图被重建",
	PressButton: "点「查看更早的过程」",
	ScrollTop: "向上滑到顶",
	MarkOlder: "确认存储里还有更早的步骤",
};

export function step(action) {
	return STEPS[action] ?? action;
}

export function cause() {
	return "视图重建时把续读位置弄丢了，界面还说有更早的步骤，点下去读到的还是同一页";
}
