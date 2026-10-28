export const readerActions = ['ReadManual', 'Disable', 'Edit', 'Delete'];
export function state(v) {
 const task = !v.row.present ? '任务已删除' : (v.row.enabled ? '任务启用' : '任务停用');
 const phase = Object.values(v.pc).filter(value => value === 1).length ? '，手动请求已读取' : '';
 const running = v.active.length ? `，${v.active.length}个执行尚未结束` : '，无在途执行';
 const writing = Object.values(v.pc).some(value => value === 3 || value === 4) ? '，等待完成提交' : '';
 const error = v.duplicate ? '，重复领取' : v.staleWrite ? '，覆盖新状态' : v.partialHistory ? '，历史与状态分离' : '';
 return task + phase + running + writing + error;
}
const labels = {ReadManual:'手动请求读取任务',ClaimManual:'手动请求尝试领取',ClaimTimer:'定时调度原子领取',ReadFinish:'执行结束读取状态',WriteHistory:'单独写入执行历史',WriteFinish:'提交完成状态',Disable:'停用后续调度',Edit:'编辑任务配置',Delete:'删除任务',Expire:'执行租约到期'};
export function step(action) { return labels[action] ?? action; }
export function cause(v, facts, invariant) { return ({ExclusiveClaim:'未失效的执行权被再次领取',NoResurrection:'删除后仍用旧快照重新插入',AtomicHistory:'历史写入没有绑定有效执行权',NoStaleCompletion:'旧执行覆盖了新的任务状态'})[invariant]; }
export const legend = {works:'正常',wrong:'原实现存在风险',fixed:'修复后不再出现'};
