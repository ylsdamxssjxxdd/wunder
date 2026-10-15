import { reactive } from 'vue';
import { filterConsumedQueueItems, resolveConsumedQueueEventIdentities, projectQueueItems, type QueuedTurn } from './chatQueueProjection';

type QueueState = {
  sessionId: string;
  items: QueuedTurn[];
  loading: boolean;
  error: string;
  revision?: number;
  rowRevisions?: Map<string,number>;
  epoch?: number;
};

const queues = new Map<string, QueueState>();
const selection = reactive({ sessionId: '' });
function queue(sessionId: string): QueueState {
  let value=queues.get(sessionId);
  if (!value) {
    value=reactive({sessionId,items:[],loading:false,error:'',revision:0,rowRevisions:new Map<string,number>()});
    queues.set(sessionId,value);
    while(queues.size>32) {
      const id=[...queues.keys()].find(id=>id!==selection.sessionId && id!==sessionId);
      if(id===undefined)break;
      queues.delete(id);
    }
  }
  return value;
}
// Compatibility facade for the selected composer; background events always
// address their own bounded session entry, never change selection.
const state: QueueState = {
  get sessionId(){return selection.sessionId;},
  set sessionId(id){selection.sessionId=id;},
  get items(){return queue(selection.sessionId).items;},
  set items(items){queue(selection.sessionId).items=items;},
  get loading(){return queue(selection.sessionId).loading;},
  set loading(loading){queue(selection.sessionId).loading=loading;},
  get error(){return queue(selection.sessionId).error;},
  set error(error){queue(selection.sessionId).error=error;}
};

// A queue-start frame is the authoritative hand-off from the composer queue
// to the live thread.  The queue endpoint is eventually consistent, so keep a
// small per-session tombstone set to stop an older response from resurrecting
// an item that has already started (or was withdrawn).
const consumedQueueIds = new Map<string, Set<string>>();
const MAX_CONSUMED_QUEUE_IDS = 128;
const MAX_CONSUMED_QUEUE_SESSIONS = 32;

const cleanText = (value: unknown): string => String(value ?? '').trim();

export const applyItems = (sessionId: string, items: QueuedTurn[]) => {
  const consumed = consumedQueueIds.get(sessionId);
  queue(sessionId).items = filterConsumedQueueItems(items, consumed || new Set<string>());
};
export const chatQueueRevision = (sessionId: string) => queue(sessionId).revision || 0;
export const chatQueueForSession = (sessionId: string) => queue(sessionId);

export const rememberQueueIdentities = (sessionId: string, identities: Iterable<string>) => {
  if (!sessionId) return;
  let set = consumedQueueIds.get(sessionId);
  if (!set) {
    set = new Set<string>();
    consumedQueueIds.set(sessionId, set);
  }
  for (const identity of identities) {
    const value = cleanText(identity);
    if (!value) continue;
    set.add(value);
  }
  while (set.size > MAX_CONSUMED_QUEUE_IDS) set.delete(set.values().next().value as string);
  while (consumedQueueIds.size > MAX_CONSUMED_QUEUE_SESSIONS) {
    consumedQueueIds.delete(consumedQueueIds.keys().next().value as string);
  }
};

export const consumeQueueItem = (sessionId: string, payload: unknown, eventType: unknown): void => {
  if (!sessionId) return;
  const identities = resolveConsumedQueueEventIdentities(
    queue(sessionId).items, eventType, payload
  );
  if (!identities.length) return;
  const known=consumedQueueIds.get(sessionId);
  if (!known || identities.some(id=>!known.has(id))) queue(sessionId).revision=chatQueueRevision(sessionId)+1;
  rememberQueueIdentities(sessionId, identities);
  const target = queue(sessionId);
  if (target.items.length) {
    const remaining = filterConsumedQueueItems(target.items, consumedQueueIds.get(sessionId) || new Set<string>());
    if (remaining.length !== target.items.length) target.items = remaining;
  }
};

export const chatQueueState = state;

/** Committed display rows also carry the queue hand-off on display-only streams. */
export function reconcileChatQueueRows(sessionId: string, rows: Array<{
  root_turn_id: string; client_message_id?: string; status: string; revision?: number;
  user?: {content:string;attachments?:unknown[]};
  queued_input?: {queue_id:string;reason:string;priority:number} | null;
}>, epoch?: number): void {
  const target=queue(sessionId);
  if(epoch !== undefined && target.epoch !== epoch) {
    target.rowRevisions!.clear();
    target.epoch=epoch;
  }
  for (const row of rows) {
    if (row.root_turn_id.startsWith('pending:') || row.status==='accepted') continue;
    const revisions=queue(sessionId).rowRevisions!;
    const revision=Number(row.revision || 0);
    if (revisions.has(row.root_turn_id) && revisions.get(row.root_turn_id)!>=revision) continue;
    revisions.set(row.root_turn_id,revision);
    while(revisions.size>128)revisions.delete(revisions.keys().next().value!);
    if (['queued','pending'].includes(row.status)) {
      if(!row.queued_input?.queue_id)continue;
      const target=queue(sessionId);
      const incoming=projectQueueItems([{
        queue_id:row.queued_input.queue_id, root_turn_id:row.root_turn_id,
        client_message_id:row.client_message_id, content:row.user?.content || '',
        attachments:row.user?.attachments || [], priority:row.queued_input.priority,
        status:'pending'
      }])[0];
      if(!incoming)continue;
      const at=target.items.findIndex(item=>item.queueId===incoming.queueId);
      if(at>=0 && target.items[at].content===incoming.content && target.items[at].priority===incoming.priority)continue;
      target.revision=chatQueueRevision(sessionId)+1;
      const items=target.items.slice();
      if(at<0)items.push(incoming); else items[at]={...items[at],...incoming};
      applyItems(sessionId,items.slice(0,64));
      continue;
    }
    consumeQueueItem(sessionId, {
      root_turn_id: row.root_turn_id,
      client_message_id: row.client_message_id
    }, 'queue_start');
  }
}
