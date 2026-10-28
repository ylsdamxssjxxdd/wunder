// Chart labels for StatusPhase, in the words of a reader who watches a thread
// run and reads the window title.

export const readerActions = [];

const PHASE = {
	idle: "没有任务",
	queued: "任务在排队",
	running: "任务正在跑",
	done: "任务已答复",
	error: "任务已失败",
};
const TITLE = {
	idle: "标题显示空闲",
	pending: "标题显示排队",
	running: "标题显示运行中",
	done: "标题显示完成",
	error: "标题显示错误",
};

export function state(v) {
	return `${PHASE[v.phase]}，${TITLE[v.shown]}`;
}

export function group(v) {
	const live = v.phase === "running" || v.phase === "queued";
	const settledBad = v.phase === "error" && v.shown !== "error";
	return live && v.shown === "idle" || settledBad
		? "标题与任务不符"
		: "标题与任务一致";
}

export const groupColors = {
	"标题与任务一致": "#e8f4ea",
	"标题与任务不符": "#fde8e8",
};

const STEPS = {
	Admit: "发送消息，请求被受理",
	Promote: "引擎接手这条请求",
	Reason: "模型开始思考",
	Queue: "请求进入模型队列",
	Request: "模型响应开始",
	Compact: "开始压缩上下文",
	Settle: "任务完成",
	Fail: "任务失败",
};

export function step(action) {
	return STEPS[action] ?? action;
}

export function cause() {
	return "这条状态的文字没有被标题的判定认识，正在跑的任务被读成了空闲";
}
