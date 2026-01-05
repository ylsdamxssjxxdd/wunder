import http from 'k6/http';
import { check, sleep } from 'k6';

// 统一压测脚本：通过 WUNDER_PROFILE 选择 quick/load/spike/soak。
const PROFILE = (__ENV.WUNDER_PROFILE || 'load').toLowerCase();
const BASE_URL = __ENV.WUNDER_BASE_URL || 'http://127.0.0.1:8000/wunder';
const QUESTION_OVERRIDE = (__ENV.WUNDER_QUESTION || '').trim();
const USER_PREFIX_OVERRIDE = (__ENV.WUNDER_USER_PREFIX || '').trim();
const FIXED_USER_ID = (__ENV.WUNDER_USER_ID || '').trim();
const LOAD_STREAM = (__ENV.WUNDER_STREAM || 'false') === 'true';
const SOAK_VUS = Number(__ENV.WUNDER_SOAK_VUS || 10);
const SOAK_DURATION = __ENV.WUNDER_SOAK_DURATION || '30m';
// 使用环境变量注入 API Key，避免压测脚本固定密钥。
const API_KEY = (__ENV.WUNDER_API_KEY || 'ylsdamxssjxxdd').trim();

// 全档位共享题库，尽量使用需要工具操作的简单任务。
const QUESTION_POOL = [
  '列出当前工作区根目录的文件和文件夹名称。',
  '创建一个hello.txt文件，内容为Hello, World!，然后读取并返回内容。',
  '创建notes.md，写入三行：line1、line2、line3，然后读取并返回内容。',
  '创建data.json，内容为包含name和role字段的数组示例，并返回文件内容。',
  '创建config.yaml，包含env: test与version: 1两行，并返回文件内容。',
  '创建logs目录（如果不存在），并在其中创建today.txt，内容为OK，然后列出logs目录文件名。',
  '创建hello.txt内容为Hello, World!，把World替换为Wunder，然后返回最终内容。',
  '创建todo.txt，写入task-1与task-2两行，然后追加一行done，返回最终内容。',
  '搜索当前工作区中包含Hello的文件名（没有就说明未找到）。',
  '搜索当前工作区中包含task-1的文件名（没有就说明未找到）。',
  '列出当前工作区中所有.txt文件名。',
  '列出当前工作区中所有.md文件名。',
  '统计当前工作区中.txt文件数量。',
  '统计当前工作区中.md文件数量。',
  '创建sample.txt，写入alpha、beta、gamma三行，然后返回第2行内容。',
  '创建info.txt，写入name=wunder与version=1两行，然后读取并返回内容。',
  '创建readme.txt，写入wunder，然后搜索包含wunder的文件名。',
  '创建data目录（如果不存在），列出其内容。',
  '创建tmp目录（如果不存在），并在其中创建keep.txt，内容为keep。',
  '创建numbers.txt，写入1到5每行一个数字，然后读取并返回内容。',
];

// 各压测档位的默认行为与阈值配置。
const PROFILES = {
  quick: {
    // 单 VU、短时长，适合开发阶段的冒烟验证。
    defaultQuestion: '列出当前工作区根目录的文件和文件夹名称。',
    defaultUserPrefix: 'k6-quick',
    useStreamEnv: false,
    allowFixedUserId: true,
    validateAnswer: false,
    options: {
      vus: 1,
      duration: '10s',
      thresholds: {
        http_req_failed: ['rate<0.01'],
      },
    },
  },
  load: {
    // 基线与负载：阶段式升压，适合压测吞吐与延迟。
    defaultQuestion: '创建一个hello.txt文件，内容为Hello, World!，然后读取并返回内容。',
    defaultUserPrefix: 'k6-user',
    useStreamEnv: true,
    allowFixedUserId: false,
    validateAnswer: true,
    options: {
      stages: [
        { duration: '30s', target: 5 },
        { duration: '1m', target: 20 },
        { duration: '30s', target: 0 },
      ],
      thresholds: {
        http_req_failed: ['rate<0.01'],
        http_req_duration: ['p(95)<2000'],
      },
    },
  },
  spike: {
    // 突发流量：短时间拉高并发后回落，观察限流与恢复能力。
    defaultQuestion: '创建hello.txt内容为Hello, World!，把World替换为Wunder，然后返回最终内容。',
    defaultUserPrefix: 'k6-spike',
    useStreamEnv: false,
    allowFixedUserId: false,
    validateAnswer: false,
    options: {
      stages: [
        { duration: '10s', target: 5 },
        { duration: '10s', target: 80 },
        { duration: '30s', target: 5 },
        { duration: '10s', target: 0 },
      ],
      thresholds: {
        http_req_failed: ['rate<0.02'],
      },
    },
  },
  soak: {
    // 稳定性：固定 VU 持续施压，重点观察长时间错误率与资源曲线。
    defaultQuestion: '创建todo.txt，写入task-1与task-2两行，然后追加一行done，返回最终内容。',
    defaultUserPrefix: 'k6-soak',
    useStreamEnv: false,
    allowFixedUserId: false,
    validateAnswer: false,
    options: {
      vus: SOAK_VUS,
      duration: SOAK_DURATION,
      thresholds: {
        http_req_failed: ['rate<0.01'],
        http_req_duration: ['p(95)<3000'],
      },
    },
  },
};

// 校验档位，避免因拼写错误导致意外配置。
const profileConfig = PROFILES[PROFILE];
if (!profileConfig) {
  throw new Error(
    `未知 WUNDER_PROFILE=${PROFILE}，可选：${Object.keys(PROFILES).join(', ')}`,
  );
}

// 计算本次压测用到的参数。
const STREAM = profileConfig.useStreamEnv ? LOAD_STREAM : false;
const USER_PREFIX = USER_PREFIX_OVERRIDE || profileConfig.defaultUserPrefix;

export const options = profileConfig.options;

// 优先使用环境变量的问题，其次从共享题库中随机选择，最后回退默认问题。
function pickQuestion(override, pool, fallback) {
  if (override) {
    return override;
  }
  if (!pool || pool.length === 0) {
    return fallback;
  }
  const index = Math.floor(Math.random() * pool.length);
  return pool[index];
}

export default function () {
  // 同一 user_id 并发会被拒绝，因此默认组合 VU 与 ITER 生成唯一用户。
  const userId =
    profileConfig.allowFixedUserId && FIXED_USER_ID
      ? FIXED_USER_ID
      : `${USER_PREFIX}-${__VU}-${__ITER}`;
  const question = pickQuestion(
    QUESTION_OVERRIDE,
    QUESTION_POOL,
    profileConfig.defaultQuestion,
  );
  const payload = JSON.stringify({
    user_id: userId,
    question,
    stream: STREAM,
  });

  const headers = { 'Content-Type': 'application/json' };
  if (API_KEY) {
    headers['X-API-Key'] = API_KEY;
  }
  const response = http.post(BASE_URL, payload, {
    headers,
    timeout: '120s',
  });

  const ok = check(response, {
    'status is 200': (res) => res.status === 200,
  });

  if (ok && profileConfig.validateAnswer && !STREAM) {
    // 仅在非流式场景验证 JSON answer，避免 SSE 长连接解析阻塞。
    check(response, {
      'response has answer': (res) => {
        try {
          const data = res.json();
          return Boolean(data && data.answer);
        } catch (err) {
          return false;
        }
      },
    });
  }

  sleep(1);
}
