/**
 * 从运行时输出行里抽取 banner URL（纯函数，无副作用）。
 *
 * 契约与外壳 src-tauri/src/banner.rs::is_loopback_host 对齐：
 * 只接受 http/https + 127.0.0.1 / localhost / [::1] + token。
 */
const BANNER = /dsh web:\s+(https?:\/\/(?:127\.0\.0\.1|localhost|\[::1\]):\d+\/\?token=[\w-]+)/;
const LOOPBACK_HOSTS = new Set(['127.0.0.1', 'localhost', '::1']);

/** 用 URL 类复核解析结果，挡住正则放过的畸形端口等无效 URL。 */
function isLoopbackUrl(value) {
  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    return false;
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false;
  const host = parsed.hostname.replace(/^\[/, '').replace(/\]$/, '').toLowerCase();
  return LOOPBACK_HOSTS.has(host);
}

/** 返回行中首个合法 banner URL；没有或无效则返回 null。 */
export function bannerUrl(line) {
  const match = line.match(BANNER);
  if (!match || !isLoopbackUrl(match[1])) return null;
  return match[1];
}
