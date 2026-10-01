// 临时诊断：复现 architecture.rs 的 strip_cfg_test_blocks + strip_comments，
// 然后指出 descriptor.rs 里究竟是哪一行让守卫报 minecraft。
// 这个文件用完就删（它本身不该留在仓库里）。
use std::fs;

fn strip_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut in_block = false;
    for line in src.lines() {
        let mut cur = line;
        if in_block {
            match cur.find("*/") {
                Some(i) => { in_block = false; cur = &cur[i + 2..]; }
                None => continue,
            }
        }
        loop {
            if let Some(i) = cur.find("/*") {
                if let Some(j) = cur[i..].find("*/") {
                    let mut s = String::from(&cur[..i]);
                    s.push_str(&cur[i + j + 2..]);
                    cur = Box::leak(s.into_boxed_str());
                    continue;
                } else {
                    in_block = true;
                    cur = &cur[..i];
                    break;
                }
            }
            break;
        }
        match cur.find("//") {
            Some(i) => out.push_str(&cur[..i]),
            None => out.push_str(cur),
        }
        out.push('\n');
    }
    out
}

fn strip_cfg_test_blocks(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < src.len() {
        match src[i..].find("#[cfg(test)]") {
            None => { out.push_str(&src[i..]); break; }
            Some(off) => {
                let attr_at = i + off;
                out.push_str(&src[i..attr_at]);
                let mut j = attr_at;
                let mut depth_seen = false;
                while j < src.len() {
                    match bytes[j] {
                        b'{' => { depth_seen = true; break; }
                        b';' => break,
                        _ => j += 1,
                    }
                }
                if !depth_seen { i = attr_at + "#[cfg(test)]".len(); continue; }
                let mut depth = 0i32;
                let mut k = j;
                while k < src.len() {
                    match bytes[k] {
                        b'{' => depth += 1,
                        b'}' => { depth -= 1; if depth == 0 { k += 1; break; } }
                        _ => {}
                    }
                    k += 1;
                }
                i = k;
            }
        }
    }
    out
}

fn main() {
    let path = std::env::args().nth(1).expect("需要文件路径");
    let src = fs::read_to_string(&path).expect("读文件");
    let production = strip_cfg_test_blocks(&src);
    let code = strip_comments(&production).to_lowercase();

    for bad in ["minecraft", "mojang", "curseforge", "modrinth", "forge", "fabric", "neoforge", "quilt", "optifine", "bedrock"] {
        if let Some(at) = code.find(bad) {
            // 找出它在**原始文件**里的行号
            let before = &code[..at];
            let line_in_stripped = before.matches('\n').count() + 1;
            // 剥注释会删字符，所以行号能对上（我们保留换行），列位置会偏。
            println!("  HIT `{bad}` at stripped-line {line_in_stripped}");
            // 打印剥后文本里那一行附近
            let lines: Vec<&str> = code.lines().collect();
            for k in line_in_stripped.saturating_sub(2)..(line_in_stripped + 1).min(lines.len()) {
                println!("      [{:>5}] {}", k + 1, lines[k].trim());
            }
        }
    }
    println!("  (done)");
}
