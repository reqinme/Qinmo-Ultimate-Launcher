//! # 真实 zip 的交叉验证（**用 deflate，不是手工构造的 stored**）
//!
//! ## 为什么这个文件必须存在
//!
//! `qul-infra/src/zip.rs` 的单测都用**手工构造的 stored zip** ——
//! 那是必要的（它让每个字节的含义都在我手里，于是边界样本能精确构造），
//! **但它有一个致命的盲区**：
//!
//! > **真实的 zip 用的是 deflate，而 mine 全是 stored。**
//!
//! 也就是说：**inflate 那条路径在单测里根本没被真实数据走过**。
//! 而 inflate 是本轮新增的代码里最容易错的一段（位序、Huffman、LZ77）。
//!
//! 所以这里用一个**由外部工具（PowerShell 的 `Compress-Archive`）产生的
//! 真实 deflate zip** 做交叉验证 —— 它的价值是**独立性**：
//! 压缩方是别的实现，所以"我们能解开它"这件事**不能被我们的实现自证**。
//!
//! ## fixture 是只读的
//!
//! `tests/fixtures-real.zip` 一旦提交就**不再改动**（它与
//! `qul-core/tests/fixtures/instances/` 的纪律相同）：
//! 它是"外部实现产出的字节"的替身，改了它整条交叉验证就失去意义。

use qul_core::zip::parse_central_directory;
use qul_infra::zip::{extract, ExtractConfig};

/// 真实 zip 的字节。
fn real_zip() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures-real.zip");
    std::fs::read(&p).unwrap_or_else(|e| {
        panic!(
            "读不到 {}：{e}\n（这个 fixture 由外部工具产生，是交叉验证的基础，不该缺失）",
            p.display()
        )
    })
}

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!(
        "qul-real-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|x| x.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn 真实_deflate_zip_能被解析且方法是_deflate() {
    // 先确认这个 fixture **确实**用的是 deflate ——
    // 否则整条交叉验证会退化成"又验了一遍 stored"。
    let z = real_zip();
    let entries = parse_central_directory(&z).expect("外部工具产生的 zip 必须能解析");
    assert!(!entries.is_empty(), "fixture 里应当有内容");
    let deflate_count = entries
        .iter()
        .filter(|e| e.method == qul_core::zip::Method::Deflate)
        .count();
    assert!(
        deflate_count > 0,
        "**这个 fixture 必须含 deflate 条目** —— 否则它验不到 inflate：{:?}",
        entries
            .iter()
            .map(|e| (e.display_name(), e.method))
            .collect::<Vec<_>>()
    );
}

#[test]
fn 真实_deflate_zip_能被完整解出且_crc_全过() {
    // **这条测试是 inflate 的端到端验收。**
    //
    // 它的强度来自"压缩方是别的实现"：
    // - 若我们的 inflate 位序错了 → 解出乱码 → **CRC 不匹配**
    // - 若我们的 Huffman 表构造错了 → 解出乱码或报错
    // - 若我们的 LZ77 复制错了 → 解出乱码
    //
    // 三种错都**必然**在这里被 CRC 拦住，而 CRC 的基准来自**外部工具的字节**。
    let z = real_zip();
    let d = tmpdir("extract");
    let out = extract(&z, &d, &ExtractConfig::default(), None).expect("应当能解压");

    assert!(
        out.skipped.is_empty(),
        "**不该有任何条目被跳过**（跳过意味着我们的解码或校验出了问题）：{:?}",
        out.skipped
    );
    assert!(out.files > 0, "应当解出了文件");
    assert!(out.bytes > 0, "应当解出了字节");

    // 内容必须与"压缩前的东西"一致 —— 而那个东西我知道：
    // `hello world\n` 重复 200 次 + 一个嵌套文件。
    let hello = std::fs::read_to_string(d.join("hello.txt"))
        .or_else(|_| std::fs::read_to_string(d.join("src").join("hello.txt")))
        .expect("应当有 hello.txt");
    // ⚠️ **第一版断言写成 `assert_eq!(hello, "hello world\n".repeat(200))`，而它红了。**
    // 差别是开头多了一个 `\u{feff}`（UTF-8 BOM）—— 因为 fixture 是用
    // PowerShell 的 `Set-Content -Encoding UTF8` 造的，而那个命令会加 BOM。
    //
    // **而这次"红"本身就是最有价值的证据**：它证明
    // ① inflate 逐字节正确（否则差别会到处都是，而不是只多一个 BOM）；
    // ② CRC 校验真的在跑（它必须先通过，我们才能读到这个文件）。
    //
    // 所以修法不是"放宽断言"，而是**把断言写成"忽略 BOM 的逐字节相等"** ——
    // 那样它仍然能发现任何一个字节的错。
    let expected = "hello world\n".repeat(200);
    let got = hello.strip_prefix('\u{feff}').unwrap_or(&hello);
    assert_eq!(
        got, expected,
        "**解出的内容必须逐字节正确**（错一个字节 CRC 就会拦住）"
    );
    assert!(got.len() > 2000, "这确实是一份被 deflate 压过的内容");

    // 嵌套的文件也要在（**它验的是"解出目录结构"而不是"解出一个文件"**）
    let nested = std::fs::read_to_string(d.join("sub").join("nested.txt"))
        .or_else(|_| std::fs::read_to_string(d.join("src").join("sub").join("nested.txt")))
        .expect("应当有嵌套文件");
    assert!(nested.contains("nested content"), "{nested}");
    assert!(nested.contains("中文"), "非 ASCII 内容也要正确：{nested}");

    // **不留 .part**（原子性的可见证据）
    assert_eq!(qul_infra::zip::sweep_parts(&d), 0, "不该留下临时文件");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn 真实_zip_的条目尺寸与解出的一致() {
    // **这条测试问的是一个具体的问题**：解析器从中央目录读出的
    // `uncompressed_size`，与我们真正 inflate 出来的长度**是否一致**。
    //
    // 它之所以值得单独测：`extract` 内部拿 `uncompressed_size + 1` 当
    // inflate 的上限，所以"声明尺寸错了"会让 inflate 提前或过晚失败，
    // 而那种失败的表现是"某个条目被跳过"，不是"尺寸不符"。
    let z = real_zip();
    let entries = parse_central_directory(&z).unwrap();
    let d = tmpdir("per-entry");
    let out = extract(&z, &d, &ExtractConfig::default(), None).unwrap();
    assert!(out.skipped.is_empty(), "{:?}", out.skipped);

    // 逐个条目：它解出来的实际长度必须等于中央目录声明的大小
    let mut checked = 0usize;
    let mut declared_total: u64 = 0;
    for e in entries.iter().filter(|e| !e.is_dir) {
        declared_total = declared_total.saturating_add(e.uncompressed_size);
        // 按名字找到解出来的文件（fixture 是平铺 + 一个 sub/）
        let name = e.display_name().replace('\\', "/");
        let p = d.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
        assert!(p.is_file(), "条目 {} 应当被解出到 {}", name, p.display());
        let actual = std::fs::metadata(&p).unwrap().len();
        assert_eq!(
            actual, e.uncompressed_size,
            "条目 {name} 声明 {} 字节、实际解出 {actual}",
            e.uncompressed_size
        );
        checked += 1;
    }
    assert!(checked > 0, "应当有非目录条目");
    assert_eq!(
        out.bytes, declared_total,
        "解出的总字节必须等于全部条目声明之和"
    );
    let _ = std::fs::remove_dir_all(&d);
}
