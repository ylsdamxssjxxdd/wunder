// Chart labels for Win32Reentry, for statechart.mjs.
// 读者：读这份图来审查蛹 UI 线程消息处理纪律的工程师。
const FRAME_NAMES = {
	ime: "输入法消息",
	sys: "普通消息",
	exec: "延迟写闭包",
	wedge: "窗口过程已卡死",
};
const CTX_NAMES = {
	none: "",
	exec: "刚发生过输入法写，位置：延迟闭包内",
	"sys-frame": "刚发生过输入法写，位置：普通消息帧内",
	"under-ime": "刚发生过输入法写，位置：输入法消息帧内",
};

export const readerActions = [];

export function state(v, facts) {
	const n = v.frames ? v.frames.length : 0;
	const chain = [];
	for (let i = 1; i <= n; i++) chain.push(FRAME_NAMES[v.frames[i].kind]);
	const parts = [chain.length ? `帧栈 ${chain.join(" → ")}` : "帧栈空，消息泵轮"];
	if (v.execPending > 0) parts.push(`待执行延迟写 ${v.execPending} 个`);
	const ctx = CTX_NAMES[v.writeCtx];
	if (ctx) parts.push(ctx);
	return parts.join("；");
}

export function group(v, facts) {
	const n = v.frames ? v.frames.length : 0;
	if (n && v.frames[n].kind === "wedge") return "线程已卡死";
	if (n >= 3) return "重入失控";
	if (n === 0) return "泵轮空闲";
	return "单帧处理中";
}
export const groupColors = {
	泵轮空闲: "#eef7ee",
	单帧处理中: "#eaf2fb",
	重入失控: "#fdeaea",
	线程已卡死: "#f8d7d7",
};

const STEPS = {
	ImeDeliversComposition: "输入法送来组合消息",
	SysDeliversMessage: "系统送来普通消息",
	RunExec: "泵轮执行延迟写闭包",
	SlintIgnoresSys: "消息与输入法无关，返回",
	SlintImeDefersWrite: "（修复后）只投递延迟写",
	SlintImeWritesInline: "（修复前）帧内直接写输入法",
	SlintSysDefersWrite: "（修复后）只投递延迟写",
	SlintSysWritesInline: "（修复前）帧内直接写输入法",
	SlintExecWrites: "延迟闭包执行输入法写",
	ReturnFromFrame: "消息帧返回",
	CaptureSyscallUnderLock: "（修复前）持锁调用捕获接口，重入再取锁",
	CaptureSyscallAfterUnlock: "（修复后）先放锁再调用捕获接口",
};
export function step(action, before, after) {
	return STEPS[action] ?? action;
}

export const legend = {
	works: "修复前后都正常",
	wrong: "修复前出错",
	fixed: "修复后已消除或新增",
	mixed: "部分修复",
	after: "仅修复后出现",
	worse: "修复后变差",
};
