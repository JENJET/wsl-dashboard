"""
扫描 Rust 代码(.rs) 和 SLint 模板 (.slint) 中使用的 i18n key，
与 en.toml 对比，找出代码中使用了但 TOML 中缺失的 key，
按段(section)分组输出到文件。

用法: python scripts/check_i18n.py
输出: scripts/i18n_missing_keys.txt
"""

import re
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent
TOML_DIR = PROJECT_ROOT / "assets" / "i18n"
REFERENCE_TOML = TOML_DIR / "en.toml"
SRC_DIR = PROJECT_ROOT / "src"
OUTPUT_FILE = PROJECT_ROOT / "target" / "i18n_missing_keys.txt"


def parse_toml_sections(filepath: Path) -> set[str]:
    sections: set[str] = set()
    section_pattern = re.compile(r'^\[([^\]]+)\]')
    with open(filepath, encoding="utf-8-sig") as f:
        for line in f:
            line = line.strip()
            m = section_pattern.match(line)
            if m:
                sections.add(m.group(1))
    return sections


def parse_toml_keys(filepath: Path) -> dict[str, str]:
    keys: dict[str, str] = {}
    current_section = ""
    section_pattern = re.compile(r'^\[([^\]]+)\]')
    kv_pattern = re.compile(r'^(\w+)\s*=\s*"')

    with open(filepath, encoding="utf-8-sig") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            m = section_pattern.match(line)
            if m:
                current_section = m.group(1)
                continue
            m = kv_pattern.match(line)
            if m and current_section:
                key_name = m.group(1)
                full_key = f"{current_section}.{key_name}"
                first_quote = line.index('"')
                last_quote = line.rindex('"')
                value = line[first_quote + 1:last_quote]
                keys[full_key] = value

    return keys


def extract_keys_from_file(filepath: Path, known_sections: set[str] | None = None, debug: bool = True) -> tuple[set[str], set[str]]:
    static_keys: set[str] = set()
    dynamic_patterns: set[str] = set()
    content = filepath.read_text(encoding="utf-8")
    rel = filepath.relative_to(PROJECT_ROOT)

    if filepath.suffix == ".rs":
        # i18n::t("...") -- single line only
        for m in re.finditer(r'i18n::t\("([^"]+)"\)', content):
            static_keys.add(m.group(1))
            if debug:
                print(f"  [i18n::t] {m.group(1)} <- {rel}")
        # i18n::tr("...", ...) -- single line
        for m in re.finditer(r'i18n::tr\("([^"]+)"\s*,', content):
            static_keys.add(m.group(1))
            if debug:
                print(f"  [i18n::tr] {m.group(1)} <- {rel}")
        # crate::i18n::t("...")
        for m in re.finditer(r'crate::i18n::t\("([^"]+)"\)', content):
            static_keys.add(m.group(1))
        # crate::i18n::tr("...", ...)
        for m in re.finditer(r'crate::i18n::tr\("([^"]+)"', content):
            static_keys.add(m.group(1))
        # get_i18n_text("...")
        for m in re.finditer(r'get_i18n_text\("([^"]+)"\)', content):
            static_keys.add(m.group(1))
        # invoke_t("...", ...)
        for m in re.finditer(r'invoke_t\("([^"]+)"', content):
            static_keys.add(m.group(1))

        # 多行: i18n::t(\n  "..."
        for m in re.finditer(r'i18n::t\(\s*\n\s*"([^"]+)"', content):
            static_keys.add(m.group(1))
            if debug:
                print(f"  [i18n::t-multi] {m.group(1)} <- {rel}")
        for m in re.finditer(r'i18n::tr\(\s*\n\s*"([^"]+)"', content):
            static_keys.add(m.group(1))
            if debug:
                print(f"  [i18n::tr-multi] {m.group(1)} <- {rel}")

        # 双引号内 x.x 结构的裸 key（如 "operation.exporting"），
        # 仅当 known_sections 不为 None 且任一前缀段在集合中时才计入。
        if known_sections is not None:
            for m in re.finditer(r'"([a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)+)"', content):
                key = m.group(1)
                if key.endswith('.exe') or key.endswith('.toml'):
                    continue
                if not key_section_in_known(key, known_sections):
                    continue
                static_keys.add(key)
                if debug:
                    print(f"  [bare-key] {key} <- {rel}")

        # 动态 key
        for m in re.finditer(r'i18n::t\(&([a-zA-Z_]\w*)\)', content):
            dynamic_patterns.add(f"i18n::t(&<variable>) -> {m.group(1)}")
        for m in re.finditer(r'i18n::tr\(&([a-zA-Z_]\w*)', content):
            dynamic_patterns.add(f"i18n::tr(&<variable>, ...) -> {m.group(1)}")
        for m in re.finditer(r'get_i18n_text\(&format!', content):
            dynamic_patterns.add("get_i18n_text(&format!(...))")
        for m in re.finditer(r'get_i18n_text\(\s*&([a-zA-Z_]\w*)\)', content):
            dynamic_patterns.add(f"get_i18n_text(&<variable>) -> {m.group(1)}")

    elif filepath.suffix == ".slint":
        for m in re.finditer(r'AppI18n\.t\("([^"]+)"', content):
            static_keys.add(m.group(1))
            if debug:
                print(f"  [AppI18n.t] {m.group(1)} <- {rel}")

        # 双引号内 x.x 结构的裸 key，仅当 known_sections 不为 None 且任一前缀段在集合中时才计入
        if known_sections is not None:
            for m in re.finditer(r'"([a-zA-Z_]\w*(?:\.[a-zA-Z_]\w*)+)"', content):
                key = m.group(1)
                if key.endswith('.exe') or key.endswith('.toml'):
                    continue
                if not key_section_in_known(key, known_sections):
                    continue
                # 跳过 import 行的路径引用（如 import { X } from "common.slint"）
                bol = content.rfind('\n', 0, m.start()) + 1
                eol = content.find('\n', m.start())
                if eol == -1:
                    eol = len(content)
                line = content[bol:eol]
                if line.strip().startswith('import'):
                    continue
                static_keys.add(key)
                if debug:
                    print(f"  [bare-key] {key} <- {rel}")

    return static_keys, dynamic_patterns


def key_section_in_known(key: str, known: set[str]) -> bool:
    """检查 key 的任一前缀段（去掉最后一项）是否在已知段集合中。"""
    parts = key.split('.')
    for i in range(len(parts) - 1):
        prefix = '.'.join(parts[:i + 1])
        if prefix in known:
            return True
    return False


def group_keys_by_section(keys: set[str]) -> dict[str, list[str]]:
    grouped: dict[str, list[str]] = {}
    for key in sorted(keys):
        parts = key.split(".")
        section = parts[0] if len(parts) >= 2 else "(root)"
        grouped.setdefault(section, []).append(key)
    return grouped


def main() -> int:
    if not REFERENCE_TOML.exists():
        print(f"错误: 找不到参考 TOML 文件: {REFERENCE_TOML}", file=sys.stderr)
        return 1

    # 1. Parse en.toml
    defined_keys = parse_toml_keys(REFERENCE_TOML)
    toml_sections = parse_toml_sections(REFERENCE_TOML)
    print(f"en.toml 已定义 key 数: {len(defined_keys)}, 段数: {len(toml_sections)}")

    # 2. 第一遍扫描：只取显式 i18n::tr/t key，从中收集代码中出现的段名
    code_sections: set[str] = set()
    for f in sorted(SRC_DIR.rglob("*.rs")):
        sk, _ = extract_keys_from_file(f, known_sections=None, debug=False)
        for k in sk:
            if '.' in k:
                code_sections.add(k.split('.')[0])
    for f in sorted(SRC_DIR.rglob("*.slint")):
        sk, _ = extract_keys_from_file(f, known_sections=None, debug=False)
        for k in sk:
            if '.' in k:
                code_sections.add(k.split('.')[0])

    # 合并 en.toml 段 + 代码段，作为裸 key 的有效段集合
    all_sections = toml_sections | code_sections
    print(f"合并段数: {len(all_sections)} (en: {len(toml_sections)}, code: {len(code_sections)})")

    # 3. 第二遍扫描：完整扫描含裸 key，使用合并后的段集合过滤
    all_static_keys: set[str] = set()
    all_dynamic: set[str] = set()

    for f in sorted(SRC_DIR.rglob("*.rs")):
        sk, dp = extract_keys_from_file(f, all_sections, debug=False)
        all_static_keys |= sk
        all_dynamic |= dp

    for f in sorted(SRC_DIR.rglob("*.slint")):
        sk, dp = extract_keys_from_file(f, all_sections, debug=False)
        all_static_keys |= sk
        all_dynamic |= dp

    print(f"代码中发现的静态 key 数: {len(all_static_keys)}")
    print(f"动态 key 模式数: {len(all_dynamic)}")

    # 3. Find missing keys
    missing = all_static_keys - defined_keys.keys()

    # 4. Group by section
    grouped_missing = group_keys_by_section(missing)

    # 5. Output
    lines = []
    lines.append("=" * 60)
    lines.append("i18n Key 缺失检查报告")
    lines.append(f"参考: {REFERENCE_TOML}")
    lines.append(f"已定义: {len(defined_keys)}")
    lines.append(f"代码静态 key: {len(all_static_keys)}")
    lines.append(f"缺失: {len(missing)}")
    lines.append("=" * 60)
    lines.append("")

    if not missing:
        lines.append("所有代码中的 key 均已定义。")
    else:
        lines.append(f"共 {len(missing)} 个缺失 key:\n")
        for section in sorted(grouped_missing.keys()):
            ks = grouped_missing[section]
            lines.append(f"  [{section}] ({len(ks)} 个)")
            for k in ks:
                lines.append(f"    {k}")
            lines.append("")

    if all_dynamic:
        lines.append("-" * 60)
        lines.append("动态 key 模式（无法静态分析）:")
        lines.append("-" * 60)
        for p in sorted(all_dynamic):
            lines.append(f"  {p}")
        lines.append("")

    # 交叉对比 en vs zh-CN
    cn_toml = TOML_DIR / "zh-CN.toml"
    if cn_toml.exists():
        cn_keys = parse_toml_keys(cn_toml)
        en_only = set(defined_keys.keys()) - set(cn_keys.keys())
        cn_only = set(cn_keys.keys()) - set(defined_keys.keys())
        lines.append("-" * 60)
        lines.append("en.toml 与 zh-CN.toml 差异:")
        lines.append("-" * 60)
        if en_only:
            lines.append(f"\nen.toml 有但 zh-CN.toml 缺失 ({len(en_only)} 个):")
            for k in sorted(en_only):
                lines.append(f"  {k}")
        if cn_only:
            lines.append(f"\nzh-CN.toml 有但 en.toml 缺失 ({len(cn_only)} 个):")
            for k in sorted(cn_only):
                lines.append(f"  {k}")
        if not en_only and not cn_only:
            lines.append("  完全一致")
        lines.append("")

        # 子集: 代码缺失的 key 里, zh-CN 有的
        missing_in_en_but_have_cn = missing & set(cn_keys.keys())
        if missing_in_en_but_have_cn:
            lines.append("-" * 60)
            lines.append(f"缺失 key 中 zh-CN 已有的 ({len(missing_in_en_but_have_cn)} 个):")
            lines.append("-" * 60)
            for k in sorted(missing_in_en_but_have_cn):
                lines.append(f"  {k} = \"{cn_keys[k]}\"")
            lines.append("")

    OUTPUT_FILE.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_FILE.write_text("\n".join(lines), encoding="utf-8")
    print(f"\n报告: {OUTPUT_FILE}")
    print(f"缺失: {len(missing)}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
