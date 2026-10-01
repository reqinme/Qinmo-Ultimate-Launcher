//! # 元数据解析的**真实数据验收**（M2）
//!
//! ## 为什么必须有这个文件
//!
//! `qul-core/src/descriptor.rs` 的 31 项单测全部用**现场手写的 JSON 片段** ——
//! 那是必要的（每个字段的语义都在我手里，边界样本能精确构造），
//! **但它有一个致命的盲区**：
//!
//! > **手写的片段是我对格式的理解，而真实元数据是格式本身。**
//! > 若我的理解错了，手写片段会跟着错，而单测照样全绿。
//!
//! 所以这里用**从 Mojang CDN 取回的原始版本 JSON** 做验收，
//! 而它们的 sha1 与官方清单**逐字节一致**（`FIXTURES.json` 记录了配对校验）。
//!
//! ## 六份夹具各自代表一种形态
//!
//! | 版本 | 形态 | 为什么是它 |
//! |---|---|---|
//! | `1.6.4` | **最小键集（11 个）** | 唯一没有 `javaVersion` / `complianceLevel` / `logging` 的样本 |
//! | `1.12.2` | 旧参数形态的**最后一个** | `minecraftArguments` 时代的末尾 |
//! | `1.13.2` | 新参数形态的**第一个** | 断代 ① 的另一侧 |
//! | `1.16.5` | **旧式 natives** | `downloads.classifiers` + `natives` 字段 |
//! | `1.19.3` | **新式 natives** | 断代 ② 的另一侧 |
//! | `26.3` | 现代形态 | Java 25 / 114 个库 |
//!
//! ## 它测的是"**我们理解的格式**"与"**真实的格式**"是否一致
//!
//! 所以每一组断言都问一个**具体**的问题，而不是"能解析出来就算过"。

use qul_core::descriptor::{ArgumentForm, Descriptor, Env, PlatformTarget};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("versions")
}

fn read(id: &str) -> String {
    let p = fixtures_dir().join(format!("{id}.json"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| {
        panic!(
            "读不到 {}：{e}\n（这份夹具是从 CDN 取回的原始字节，不该缺失）",
            p.display()
        )
    })
}

fn load(id: &str) -> Descriptor {
    Descriptor::parse(&read(id)).unwrap_or_else(|e| panic!("{id} 应当能解析：{e}"))
}

fn win() -> Env {
    // 用**本机真实的**平台值，而不是编一个 —— 那样验的才是真环境。
    Env::new(PlatformTarget::windows("10.0.26200", "x86_64"))
}

// ───────────────────────── 夹具本身的完整性 ─────────────────────────

#[test]
fn every_fixture_matches_the_official_sha1() {
    // ⚠️ **这条是整个验收的前提。**
    //
    // 若夹具被人改过（哪怕一个字节），"真实数据验收"就退化成
    // "对着我们自己改过的数据验收" —— 而那比没有验收更坏，
    // 因为它会给出一个虚假的保证。
    //
    // `FIXTURES.json` 里的 `manifest_sha1` 是官方清单声明的值。
    // 而清单本身也在这台机器上（`version_manifest_v2.json`），
    // 所以这个配对是可独立复核的。
    let idx = std::fs::read_to_string(fixtures_dir().join("FIXTURES.json")).expect("FIXTURES.json");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&idx).expect("FIXTURES.json 应当是合法 JSON");
    assert_eq!(rows.len(), 7, "应当有 7 份夹具");

    for r in &rows {
        let id = r["id"].as_str().unwrap();
        let want = r["manifest_sha1"].as_str().unwrap();
        let bytes = std::fs::read(fixtures_dir().join(format!("{id}.json"))).unwrap();
        let got = sha1_hex(&bytes);
        assert_eq!(
            got, want,
            "夹具 {id} 的 sha1 与官方清单不符 —— **它被改过**，\
             而那会让这个文件的全部验收失去意义"
        );
    }
}

/// 纯 Rust 的 SHA-1（测试专用；生产实现是 `qul-infra/src/check.rs`）。
///
/// **为什么在这里再写一遍而不是引 `qul-infra`**：`qul-core` 的集成测试
/// **不该依赖 `qul-infra`** —— 那会让"内核零 IO"这条约束在测试侧被绕过。
fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let ml = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let tmp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = tmp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha1_helper_is_correct() {
    // 自证：一个错的 SHA-1 会让上面那条测试**永远通过**（两边都是错的）。
    assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
}

// ───────────────────────── 断代 ①：参数形态 ─────────────────────────

#[test]
fn argument_form_boundary_is_where_the_probe_said_it_was() {
    // 实测结论：1.12.2 是旧形态的最后一个，1.13.2 是新形态的第一个。
    // **这条断言把那个结论钉在真实字节上**，而不是钉在分析脚本的输出上。
    let legacy = load("1.12.2");
    assert_eq!(
        legacy.argument_form().unwrap().which(),
        "legacy",
        "1.12.2 应当是旧形态"
    );
    assert!(legacy.has_legacy_arguments());
    assert!(legacy.arguments.is_none());

    let modern = load("1.13.2");
    assert_eq!(
        modern.argument_form().unwrap().which(),
        "modern",
        "1.13.2 应当是新形态（**这就是那个断代点**）"
    );
    assert!(!modern.has_legacy_arguments());
    let args_block = modern.arguments.as_ref().unwrap();
    assert!(!args_block.jvm.is_empty(), "新形态该有 jvm 参数");
    assert!(!args_block.game.is_empty(), "新形态该有 game 参数");
}

#[test]
fn legacy_arguments_split_into_usable_argv() {
    let d = load("1.6.4");
    match d.argument_form().unwrap() {
        ArgumentForm::Legacy { legacy_arguments } => {
            let argv = qul_core::descriptor::split_legacy_arguments(&legacy_arguments);
            assert!(!argv.is_empty());
            // 实测这个字符串里带占位符
            assert!(
                argv.iter().any(|a| a.starts_with("${")),
                "旧形态该含占位符：{argv:?}"
            );
        }
        other => panic!("1.6.4 应当是旧形态：{other:?}"),
    }
}

#[test]
fn modern_jvm_contains_the_natives_related_flags() {
    // 26.3 的 JVM 参数里应当有那些指向 natives 目录的 -D 开关 ——
    // 它们是 S7 实测到的官方命令行的一部分（`-Djava.library.path=…`）。
    let d = load("26.3");
    match d.argument_form().unwrap() {
        ArgumentForm::Modern { jvm, .. } => {
            let all: Vec<String> = jvm
                .iter()
                .flat_map(|i| match i {
                    qul_core::descriptor::ArgItem::Plain(s) => vec![s.clone()],
                    qul_core::descriptor::ArgItem::Conditional { value, .. } => {
                        value.as_slice().iter().map(|s| s.to_string()).collect()
                    }
                })
                .collect();
            let joined = all.join(" ");
            assert!(
                joined.contains("java.library.path"),
                "该有 java.library.path（S7 实测里它指向 natives 目录）：{joined}"
            );
        }
        other => panic!("{other:?}"),
    }
}

// ───────────────────────── 断代 ②：natives 形态 ─────────────────────────

#[test]
fn natives_form_boundary_is_where_the_probe_said_it_was() {
    // ⚠️ **这条是本文件最有价值的一条。**
    // 手写单测能验"两种形态各自被正确处理"，但**验不了"哪一种属于哪个版本"**。
    // 只有真实字节能回答后者。
    let old = load("1.16.5");
    let old_natives = old.libraries.iter().filter(|l| l.natives.is_some()).count();
    assert!(
        old_natives > 0,
        "1.16.5 该有旧式 natives（`natives` 字段）—— 实测 16 条"
    );

    let new = load("1.19.3");
    let new_old_style = new.libraries.iter().filter(|l| l.natives.is_some()).count();
    let new_standalone = new
        .libraries
        .iter()
        .filter(|l| l.name.contains("natives-windows"))
        .count();
    assert_eq!(
        new_old_style, 0,
        "1.19.3 不该再有旧式 natives —— **这就是那个断代点**"
    );
    assert!(
        new_standalone > 0,
        "1.19.3 该有新式独立 natives 条目 —— 实测 42 条"
    );

    // 而 26.3 更彻底：连 `downloads.classifiers` 都没了
    let modern = load("26.3");
    let with_classifiers = modern
        .libraries
        .iter()
        .filter(|l| l.downloads.classifiers.is_some())
        .count();
    assert_eq!(with_classifiers, 0, "26.3 不该有 classifiers 了");
}

#[test]
fn both_natives_forms_produce_a_natives_plan_on_windows() {
    // 两套形态都要能在 **windows** 上产出 natives —— 这是 M3 真正需要的结论。
    for id in ["1.16.5", "1.19.3", "26.3"] {
        let d = load(id);
        let plans = d.library_plans(&win());
        let with_natives = plans.iter().filter(|p| p.natives.is_some()).count();
        assert!(
            with_natives > 0,
            "{id} 在 windows 上应当产出 natives（两套形态都算）：\
             plans={} 而 natives=0",
            plans.len()
        );
    }
}

#[test]
fn windows_natives_are_never_the_linux_or_macos_ones() {
    // ⚠️ 一个"按名字包含 natives 就收"的实现会把 **linux 与 macos 的 natives
    // 也一起装上**。而那是**静默的**：装了不报错，只是多下了几十 MB，
    // 且 classpath / library.path 里混进了错平台的二进制。
    for id in ["1.16.5", "1.19.3", "26.3"] {
        let d = load(id);
        for p in d.library_plans(&win()) {
            if let Some(n) = &p.natives {
                let path = n.path.to_lowercase();
                let name = p.name.to_lowercase();
                // 判据用**实际取到的路径与名字**，不用"库名里有没有 natives"
                assert!(
                    !path.contains("natives-linux") && !path.contains("natives-macos"),
                    "{id}: {} 取到了错平台的 natives：{}",
                    p.name,
                    n.path
                );
                assert!(
                    !name.contains("natives-linux") && !name.contains("natives-macos"),
                    "{id}: 选中了错平台的条目 {}",
                    p.name
                );
            }
        }
    }
}

// ───────────────────────── 缺失字段的实测 ─────────────────────────

#[test]
fn the_minimal_version_really_is_minimal() {
    // 实测：1.6.4 有 11 个顶层键，且是唯一没有 javaVersion / complianceLevel 的样本。
    let raw = read("1.6.4");
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let keys = v.as_object().unwrap().len();
    assert_eq!(keys, 11, "1.6.4 应当是 11 个顶层键，实测如此");

    let d = load("1.6.4");
    assert!(d.java_version.is_none(), "1.6.4 没有 javaVersion");
    assert!(d.compliance_level.is_none(), "1.6.4 没有 complianceLevel");
    assert!(d.logging.is_none(), "1.6.4 没有 logging");

    // 而它必须把这三件事**列成假设**，不是静默回退
    let fields: Vec<&str> = d.assumptions().iter().map(|a| a.field).collect();
    for want in ["javaVersion", "complianceLevel", "logging"] {
        assert!(
            fields.contains(&want),
            "1.6.4 的假设清单里该有 {want}：{fields:?}"
        );
    }
}

#[test]
fn java_major_version_climbs_across_eras_as_measured() {
    // 实测的爬升：8 → 8 → 8 → 16 → 17 → 25。
    // 一个把 Java 版本写死的实现会在这里立刻暴露。
    let cases = [
        ("1.6.4", None),
        ("1.12.2", Some(8)),
        ("1.16.5", Some(8)),
        ("1.19.3", Some(17)),
        ("26.3", Some(25)),
    ];
    for (id, want) in cases {
        let d = load(id);
        let got = d.java_version.as_ref().and_then(|j| j.major_version);
        assert_eq!(got, want, "{id} 的 javaVersion.majorVersion");
    }
}

#[test]
fn asset_index_id_is_data_not_a_version_number() {
    // ⚠️ 实测取值：`pre-1.6` / `legacy` / `1.12` / `2` / `19` / `34`。
    // **它看起来像版本号但不是。** 一个"用版本 id 拼出索引名"的实现
    // 在 `pre-1.6` 与 `legacy` 上会错，在 `2` / `19` / `34` 上更会错。
    // ⚠️ **这一组期望被实测纠正过一次。**
    //
    // 我原先写 `1.6.4 → "pre-1.6"`，而实测是 **`legacy`** ——
    // `pre-1.6` 属于更早的 alpha/beta 那一档（`b1.7.3` 才是）。
    //
    // 这恰好是这条测试要证明的事：**索引 id 是数据，不能从任何东西推**，
    // 而我试图凭记忆推它，就错了。
    let cases = [
        ("b1.7.3", "pre-1.6"),
        ("1.6.4", "legacy"),
        ("1.19.3", "2"),
        ("26.3", "34"),
    ];
    for (id, want) in cases {
        let d = load(id);
        let got = d.asset_index_ref().map(|a| a.id.as_str());
        assert_eq!(got, Some(want), "{id} 的 assetIndex.id");
    }
    // 而 `id` 与版本 id 的**关系是不存在的**
    let d = load("26.3");
    let idx = d.asset_index_ref().unwrap();
    assert_ne!(idx.id, d.id, "索引 id 与版本 id 是两件事");
}

#[test]
fn main_class_changed_across_eras_and_is_always_read_from_json() {
    // 实测：`RubyDung` → `launchwrapper.Launch` → `client.main.Main`。
    // ⚠️ 断言用**结构性质**而不是抄那三个类名。
    //
    // 理由有两条，而第二条是这一轮学到的：
    //   1. 这条测试要证明的是"**mainClass 必须从 JSON 读**"，
    //      而那由"不同年代的版本给出不同的值"就能证明；
    //   2. 抄类名会把**产品名**带进内核的测试代码位置 ——
    //      而架构守卫刚刚（正确地）因此拦下了我一次。
    //
    // 所以判据是：**同一时代的两个版本给出同一个值，而跨时代给出不同的值。**
    // ⚠️ **这一条也失败过一次，而那次失败暴露的是"夹具选得不够"。**
    //
    // 我原先断言"`1.6.4` 与 `26.3` 的 mainClass 不同" —— 而它**相同**。
    // 原因：从 `1.6.4` 起一直是同一个类，**真正的变化在更早**。
    // 也就是说，**只拿现代夹具的话，"mainClass 必须从 JSON 读"这条测不出来**
    // （硬编码成那一个值也能全绿）。
    //
    // 所以补了 `b1.7.3` 作为夹具 —— 它的 mainClass 与 1.6.4+ **确实不同**。
    let beta = load("b1.7.3");
    let old_a = load("1.6.4");
    let old_b = load("1.12.2");
    let modern = load("26.3");
    assert_ne!(
        beta.main_class().unwrap(),
        old_a.main_class().unwrap(),
        "**跨年代该不同** —— 这条断言是 `mainClass` 必须从 JSON 读的证据"
    );
    assert_eq!(
        old_a.main_class().unwrap(),
        old_b.main_class().unwrap(),
        "同一时代的两个版本该有同一个 mainClass"
    );
    assert_eq!(
        old_b.main_class().unwrap(),
        modern.main_class().unwrap(),
        "而从 1.6.4 起它一直没变 —— 这本身也是一个实测事实"
    );
    // 四个都是非空的合格类名
    for id in ["b1.7.3", "1.6.4", "1.12.2", "26.3"] {
        let mc = load(id).main_class().expect("该有 mainClass").to_string();
        assert!(mc.contains('.'), "{id} 的 mainClass 看起来不像类名：{mc}");
        assert!(!mc.contains(' '), "{id} 的 mainClass 不该含空格：{mc}");
    }
}

// ───────────────────────── Java 需求：**事实 vs 假设** ─────────────────────────

#[test]
fn java_requirement_comes_from_the_metadata_when_declared() {
    // ⚠️ **这条把 M2 的两块接起来**：`descriptor.rs` 的 `javaVersion`
    // 与 `java.rs` 的 `JavaRequirement`。
    //
    // 实测的爬升：1.6.4 无声明 → 1.12.2 声明 8 → 1.16.5 声明 8
    //            → 1.19.3 声明 17 → **26.3 声明 25**
    let cases = [
        ("1.12.2", 8u32),
        ("1.16.5", 8),
        ("1.19.3", 17),
        ("26.3", 25),
    ];
    for (id, want) in cases {
        let d = load(id);
        let r = d.java_requirement();
        assert!(
            r.source.is_declared(),
            "{id} 的详情里声明了 javaVersion，所以来源该是「事实」而不是推断：{:?}",
            r.source
        );
        assert_eq!(r.declared_major, Some(want), "{id} 声明的 majorVersion");
        assert_eq!(r.requirement.min_major(), want, "{id} 的最低主版本");
    }
}

#[test]
fn java_requirement_is_marked_as_a_guess_when_not_declared() {
    // 实测：`1.6.4` **完全没有** `javaVersion`。
    // 而这里要断言的是**它被标成假设**，不是"它猜对了"。
    let d = load("1.6.4");
    let r = d.java_requirement();
    assert!(!r.source.is_declared(), "1.6.4 没声明，所以不该标成事实");
    assert_eq!(r.declared_major, None);
    assert_eq!(r.source.as_str(), "guessed-from-version-table");
    // 而推断的结果是 Java 8（1.6.4 属于 ≤1.16.5 那一档）
    assert_eq!(r.requirement.min_major(), 8);
    // **解释里必须说清"这是假设"**
    let e = r.explain();
    assert!(e.contains("假设"), "解释要说清它是假设：{e}");
}

#[test]
fn a_declared_25_is_not_rounded_down_to_a_named_tier() {
    // ⚠️ **这是本轮新加的那条语义，而它值得单独一条测试。**
    //
    // `26.3` 声明 majorVersion=25。一个"看到 25 就归到最近的 Java21 档"的
    // 实现会让需求**被低估** —— 后果是选出一个版本过低的 Java，
    // 然后游戏以一个难查的方式失败。
    //
    // 所以断言的是**精确的 25**，而不是"至少 21"。
    let d = load("26.3");
    let r = d.java_requirement();
    assert_eq!(
        r.requirement.min_major(),
        25,
        "**不许取整** —— 声明 25 就要 25"
    );
    assert!(
        r.requirement.human().contains("25"),
        "{}",
        r.requirement.human()
    );
    // 而它确实**不是**那个命名的 Java21 档
    assert_ne!(
        r.requirement,
        qul_core::java::JavaRequirement::Java21,
        "25 与「Java 21 档」是两件事"
    );
}

#[test]
fn the_version_table_does_not_pretend_to_know_the_future() {
    // 表的兜底在 1.21 起改用 `AtLeast(n)`。断言两件事：
    //   ① 1.21.x 仍然是「Java 21 档」（实测 1.21.4 声明 21）
    //   ② 而更高的次版本号**不再被钉死**
    use qul_core::java::{GameVersion, JavaRequirement};
    assert_eq!(
        GameVersion::parse("1.21.4").unwrap().requirement(),
        JavaRequirement::Java21
    );
    assert_eq!(
        GameVersion::parse("1.24.0").unwrap().requirement(),
        JavaRequirement::Java21
    );
    assert_eq!(
        GameVersion::parse("1.25.0").unwrap().requirement(),
        JavaRequirement::AtLeast(25),
        "1.25 的要求就是 25 —— 表不再假装知道"
    );
    // 而 2.x 也**不猜**
    assert_eq!(
        GameVersion::parse("2.0.0").unwrap().requirement(),
        JavaRequirement::AtLeast(21)
    );
}
// ───────────────────────── 库计数的实测 ─────────────────────────

#[test]
fn library_counts_match_what_was_measured() {
    // 实测：1.6.4=21 / 1.12.2=39 / 1.19.3=89 / 26.3=114。
    // 而**在 windows 上被规则保留的**是另一个数 —— 两者都要对上。
    let cases = [
        ("1.6.4", 21usize),
        ("1.12.2", 39),
        ("1.19.3", 89),
        ("26.3", 114),
    ];
    for (id, want) in cases {
        let d = load(id);
        assert_eq!(d.libraries.len(), want, "{id} 声明的库数");
    }
    // windows 上：26.3 应当恰好是 74（这正是 S7 那一轮实测出来的数）
    let d = load("26.3");
    let plans = d.library_plans(&win());
    assert_eq!(
        plans.len(),
        74,
        "26.3 在 windows 上应当有 74 个库 —— 与 S7 实测的 `libraries` 74 文件吻合"
    );
}

#[test]
fn no_plan_has_both_no_artifact_and_no_natives() {
    // 一个"既没有 jar 也没有 natives"的库计划是无意义的 ——
    // 它要么说明过滤错了，要么说明这个库在元数据里是空的。
    for id in ["1.6.4", "1.12.2", "1.19.3", "26.3"] {
        let d = load(id);
        for p in d.library_plans(&win()) {
            assert!(
                p.artifact.is_some() || p.natives.is_some(),
                "{id}: 库 {} 的产物为空（既无 jar 也无 natives）",
                p.name
            );
        }
    }
}
