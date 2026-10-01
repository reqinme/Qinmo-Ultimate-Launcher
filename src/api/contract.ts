/**
 * 与 Rust 侧的能力描述符契约（方案 §3.3）。
 *
 * **这是"前端零业务逻辑"的落点**：界面拿到的不是"该不该显示"的判断结果，
 * 而是**一组已经算好的结论**（`{ enabled, reason }`）。界面只负责渲染，
 * **不做判断**——判断属于 Provider。
 *
 * 因此这里没有 `isJava()`、`supportsMods()` 之类的函数，也**不该有**。
 * 它们一旦出现，就意味着界面开始认识具体产品了，而那正是要消灭的东西。
 */

/**
 * 后端的能力结论。
 *
 * `reason` 在 `enabled === false` 时**必为字符串**——这个不变式在 Rust 侧
 * 由类型系统保证（见 `crates/qul-core/src/caps.rs`），在前端由
 * [`assertCapabilitiesValid`] 在边界上兜住。**两侧都守，是因为 JSON 不认类型。**
 */
export type Capability =
  | { readonly enabled: true }
  | { readonly enabled: false; readonly reason: string };

export type Capabilities = Readonly<Record<string, Capability>>;

/** 契约被破坏（后端给了没有原因的禁用态，或前端手写了脏数据）。 */
export class CapabilityContractError extends Error {
  /**
   * 出问题的能力名。
   *
   * **用显式字段而不是构造函数参数属性**（`constructor(readonly key: string)`）：
   * 参数属性虽合法，但它把"声明"和"赋值"藏在签名里，且部分解析器/降级目标
   * 对它支持不一致。显式声明零成本，还省掉一层解释。
   */
  readonly key: string;

  constructor(key: string, message: string) {
    super(message);
    this.name = "CapabilityContractError";
    this.key = key;
  }
}

/**
 * 校验任意来源的 `Capabilities`。
 *
 * **为什么前端还要再验一遍**：Rust 侧的类型系统管不到穿越 IPC 的 JSON，
 * 也管不到测试里手写的桩数据。这是**边界校验**，不是重复劳动。
 *
 * @throws {CapabilityContractError} 当任一禁用态缺少非空原因时
 */
export function assertCapabilitiesValid(caps: Capabilities): void {
  for (const [key, cap] of Object.entries(caps)) {
    if (cap.enabled) continue;
    if (typeof cap.reason !== "string" || cap.reason.trim() === "") {
      throw new CapabilityContractError(
        key,
        `能力 \`${key}\` 处于禁用态但没有给出原因。` +
          `禁用态必须带人话原因（方案 §3.3 规则 3）——没有原因的禁用态一律视为缺陷。`,
      );
    }
  }
}

/**
 * 从任意 JSON 解析出 `Capabilities`，**先验后用**。
 *
 * 供 Tauri 命令的返回边界使用；解析与校验都通过才交给界面。
 */
export function parseCapabilities(raw: unknown): Capabilities {
  if (raw === null || typeof raw !== "object" || Array.isArray(raw)) {
    throw new CapabilityContractError("(root)", "能力表应是一个对象");
  }
  const caps = raw as Capabilities;
  assertCapabilitiesValid(caps);
  return caps;
}
