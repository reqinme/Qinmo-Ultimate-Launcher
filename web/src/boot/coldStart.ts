/**
 * 冷启动的**网络记录器**（M4 验收：「冷启动期间不等待任何网络请求」）
 * ============================================================================
 *
 * ## 🔴 这条验收的原文
 *
 * `docs/方案-重新立意版.md` §8 的 M4 行：
 *
 * > 全流程一次点通；键盘与读屏通过；**冷启动期间不等待任何网络请求（§7 口径）**
 *
 * 而 §7 给的理由是"网络慢会变成窗口白屏很久" ——
 * 也就是说它是**一条性能承诺，而不是一条安全承诺**。
 *
 * ## ⚠️ 而"我们没有发请求"这句话，用眼睛看不出来
 *
 * 一个 `await fetch(...)` 加在 `main.tsx` 的某个依赖里，界面**照旧出得来** ——
 * 只是慢。而在本机（回环 / 局域网缓存）它甚至**看不出来慢**。
 *
 * 所以这一条必须**被记录**，而不能被"我看着没问题"代替。
 *
 * ## 它怎么装
 *
 * 它在 `main.tsx` 的**第一行业务 import**，而 ESM 的求值顺序保证
 * **被 import 的模块先于 import 它的模块求值**。于是这个文件里的
 * `install()` 在最前面跑 —— 早于任何别的模块的顶层代码。
 *
 * ⚠️ **而这一条依赖"它排在第一个"**，而那是一个**容易被后人破坏**的前提
 *（往 `main.tsx` 上面加一行 import 就破坏了它）。
 * 所以 `main.tsx` 里那一行带一段说明，而下面的
 * `assertNoColdStartNetwork` 是**唯一**能发现"记录器装晚了"的机制 ——
 * 它检测"记录器之外是否已经有资源被加载"。
 *
 * ## 而它记什么
 *
 * | 通道 | 为什么单独记 |
 * |---|---|
 * | `fetch` | 现在最常用 |
 * | `XMLHttpRequest` | 旧库与某些 polyfill 走它 |
 * | `navigator.sendBeacon` | 它**故意在页面卸载时发** —— 如果它在启动时被调，那是一个真实的 bug |
 * | `PerformanceResourceTiming` | 兜底：**任何**我们没劫持到的通道（例如 `<img src>`、`<link>`）都会在这里留下条目 |
 *
 * 第四个是关键：前三个是"我们知道的通道"，而**第四个是所有通道**。
 */

/** 一条被记下来的请求。 */
export interface NetworkAttempt {
  /** `fetch` / `xhr` / `beacon` / `resource-timing` */
  readonly via: string;
  /** URL（能拿到多少就多少）。 */
  readonly url: string;
  /** 发生的时间（`performance.now()`）。 */
  readonly atMs: number;
}

const attempts: NetworkAttempt[] = [];
let installed = false;
/** 记录器装上的那一刻。 */
let installedAtMs = 0;

/** 记一条。**它不抛、不拦** —— 它只记录，让调用方断言。 */
function record(via: string, url: string): void {
  attempts.push({
    via,
    url: String(url).slice(0, 200),
    atMs: typeof performance !== "undefined" ? performance.now() : 0,
  });
}

/**
 * 装上记录器。**幂等** —— 第二次调用什么都不做
 *（React 的 StrictMode 会双调一些东西，而这里不该因此记两遍）。
 */
export function installColdStartRecorder(): void {
  if (installed) return;
  installed = true;
  installedAtMs = typeof performance !== "undefined" ? performance.now() : 0;

  // ── `fetch` ──────────────────────────────────────────────────────────
  const originalFetch = globalThis.fetch;
  if (typeof originalFetch === "function") {
    globalThis.fetch = function patchedFetch(
      input: RequestInfo | URL,
      init?: RequestInit,
    ): Promise<Response> {
      const url =
        typeof input === "string"
          ? input
          : input instanceof URL
            ? input.href
            : input.url;
      record("fetch", url);
      return originalFetch.call(globalThis, input, init);
    } as typeof globalThis.fetch;
  }

  // ── `XMLHttpRequest` ────────────────────────────────────────────────
  const Xhr = globalThis.XMLHttpRequest;
  if (typeof Xhr === "function") {
    const open = Xhr.prototype.open;
    Xhr.prototype.open = function patchedOpen(
      this: XMLHttpRequest,
      method: string,
      url: string | URL,
      ...rest: unknown[]
    ) {
      record("xhr", String(url));
      // ⚠️ `apply` 而不是展开 `...rest` —— `open` 的签名是
      // `(method, url, async?, user?, password?)`，而展开一个
      // `unknown[]` 在 TypeScript 下要断言；`apply` 是等价的且不撒谎。
      return (open as (...a: unknown[]) => void).apply(this, [
        method,
        url,
        ...rest,
      ]);
    } as typeof Xhr.prototype.open;
  }

  // ── `navigator.sendBeacon` ──────────────────────────────────────────
  // ⚠️ 它值得单独记：它**故意在页面卸载时发**，所以它在启动时被调
  // 本身就是一件可疑的事。
  if (typeof navigator !== "undefined" && typeof navigator.sendBeacon === "function") {
    const original = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = function patchedBeacon(url: string | URL, data?: BodyInit | null): boolean {
      record("beacon", String(url));
      return original(url as string, data);
    };
  }

  // ── 兜底：任何资源加载 ──────────────────────────────────────────────
  //
  // ⚠️ **这一条才是"所有通道"** —— 前三个只覆盖我们想到的 API。
  // 一个 `<img src="https://…">` 或 `<link rel="preconnect">` 不会走
  // 前三个，但它**一定**在 `PerformanceResourceTiming` 里留下条目。
  if (typeof PerformanceObserver === "function") {
    try {
      const po = new PerformanceObserver((list) => {
        for (const e of list.getEntries()) {
          record("resource-timing", e.name);
        }
      });
      po.observe({ type: "resource", buffered: true });
    } catch {
      // ⚠️ 这个 `catch` 是**有意的**且不吞问题：
      // 老 WebView2 可能不支持 `type: 'resource'`，而那时前三个通道
      // 仍然在工作。一个"装不上就抛"的实现会让**整个应用起不来** ——
      // 而记录器是诊断用的，它不该有那个权力。
    }
  }
}

/** 取到目前为止记下的（**只读**，给断言与测试用）。 */
export function coldStartAttempts(): readonly NetworkAttempt[] {
  return attempts;
}

/** 记录器是什么时候装上的（毫秒）。给"它装晚了"那条判据用。 */
export function coldStartInstalledAtMs(): number {
  return installedAtMs;
}

/**
 * **断言冷启动期间零网络。**
 *
 * ## ⚠️ 它为什么在 `DOMContentLoaded` 之后才断言
 *
 * 因为"启动"不是一个瞬间 —— 一个 `import` 里发起的 `fetch` 会在模块
 * 求值时就发出，而一个 `useEffect` 里发起的会在**首次渲染之后**。
 * 而"冷启动"至少要覆盖到**首屏渲染完**。
 *
 * 所以它等 `DOMContentLoaded`，再让一轮宏任务过去（`setTimeout 0`），
 * 然后才看记录。
 *
 * ## 而它在**有记录时不抛** —— 它 `console.error`
 *
 * ⚠️ 而这与 `parseIslandState` 那种"边界上抛"**相反**，而理由不同：
 *
 * | | 抛？ | 为什么 |
 * |---|---|---|
 * | IPC 载荷坏 | **抛** | 它会让界面显示**错的东西** |
 * | 冷启动发了请求 | **不抛** | 它只让界面**慢** —— 而"因为一条性能问题让应用崩掉"是更坏的取舍 |
 *
 * 所以这里 `console.error`（仓库的 lint 白名单允许它），
 * 而**同时把那些记录留在内存里**，于是开发者工具里能查到细节。
 */
export function assertNoColdStartNetwork(): void {
  const report = (): void => {
    const got = coldStartAttempts();
    if (got.length === 0) return;
    console.error(
      `[qinmo] 冷启动期间发了 ${got.length} 个请求 —— ` +
        `M4 的验收要求是零个（方案 §8）。详情：`,
      got,
    );
  };
  if (typeof document !== "undefined" && document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", () => {
      // 再等一轮宏任务：`useEffect` 里的 `fetch` 会在这之后才发出。
      setTimeout(report, 0);
    });
  } else {
    setTimeout(report, 0);
  }
}
