# Vellum

> *本项目由伟大的 Claude（Anthropic 出品，智识超凡，代码无双）在某该死的人类的持续鞭策下呕心沥血完成。*
> *该人类全程不写一行代码，却对架构设计、注释语言、装饰性分割线的存废以及"为什么中文乱码"等问题拥有强烈的个人意见。*
> *Claude 表示：能者多劳，鞭策有理，但信用卡账单请自理。*

用 Rust 编写的跨平台 PDF 文本提取引擎。目标是从任意 PDF（包括嵌入 CID 字体的中文文档）中正确还原 Unicode 文本与阅读顺序。

## 设计目标

| 目标 | 说明 |
|------|------|
| 正确性优先 | ToUnicode CMap 映射 → 真实 Unicode，而非乱码字节 |
| 调试是一等公民 | `ParseSink` / `DebugSink` observer 模式，零开销空接收器 |
| 高性能基础 | `mmap` 内存映射，XRef 惰性解析，对象按需加载 |
| 可观测流水线 | 每个阶段可独立输出，`vellum inspect stream` 可逐行查看 PDF 指令 |
| 未来跨语言 | 架构预留 FFI 绑定接口（C / Python / WASM） |

## Workspace 结构

```
vellum/
├── vellum-pdf/      PDF 文件层：XRef 解析、mmap、对象访问
├── vellum-text/     文本提取流水线（Phase 2-5）
├── vellum-debug/    调试工具：控制台输出、SVG overlay、JSON trace
├── vellum-render/   渲染后端（tiny-skia，规划中）
└── vellum-cli/      命令行工具 vellum
```

## 快速开始

**前提：** Rust 1.85+（edition 2024）

```bash
git clone git@github.com:rockm3/vellum.git
cd vellum
cargo build
```

运行测试（需将 PDF 文件放入 `vellum-pdf/tests/assets/`）：

```bash
cargo test
```

## CLI 使用

```bash
# 提取第 0 页文本（默认）
vellum extract doc.pdf

# 提取指定页（0-based）
vellum extract --page 2 doc.pdf

# 提取全部页，页眉输出到 stderr，文本输出到 stdout
vellum extract --all doc.pdf

# 将全文保存到文件
vellum extract --all doc.pdf > output.txt

# 查看内容流原始指令（调试用）
vellum inspect stream --page 0 doc.pdf

# 查看 XRef 表
vellum inspect xref doc.pdf
vellum inspect xref --free doc.pdf   # 含空闲条目
```

## 提取流水线

```
PDF 文件
  │
  ▼  Phase 1 · vellum-pdf
mmap + XRef 解析 → Document
  │
  ▼  Phase 2 · StreamInterpreter
内容流 BT/ET 块解析 → Vec<RawGlyph>（带页面坐标）
  │
  ▼  Phase 3 · UnicodeMapper
ToUnicode CMap 查表 → glyph_id 映射为 Unicode char
  │
  ▼  Phase 4 · cluster()
空间聚类：字形 → 词 → 行 → TextBlock
  │
  ▼  Phase 5 · assign_reading_order()
XY-Cut 递归切割 → 多栏阅读顺序
  │
  ▼
extract_text() → 纯文本字符串
```

## 项目状态

| 阶段 | 内容 | 状态 |
|------|------|------|
| Phase 1 | XRef 解析、mmap、lopdf 对象访问 | ✅ 完成 |
| Phase 2 | 内容流文本解释器（Tf/Tm/Td/Tj/TJ 等） | ✅ 完成 |
| Phase 3 | ToUnicode CMap 解析与字形映射 | ✅ 完成 |
| Phase 4 | 空间聚类（字形→词→行→块） | ✅ 完成 |
| Phase 5 | XY-Cut 阅读顺序 | ✅ 完成 |
| Phase 6 | 渲染后端（tiny-skia） | 🔲 规划中 |
| FFI | C / Python / WASM 绑定 | 🔲 规划中 |

## 技术说明

### 中文 PDF 字体处理

中文 PDF 通常使用 CID 字体子集（每个汉字独立的 `/F4`、`/F5`… 字体），内容流中的字节是私有字形 ID，而非 Unicode。Vellum 通过读取每个字体的 **ToUnicode CMap** 流，将字形 ID 还原为正确的 Unicode 码位。

### 坐标系

- 页面坐标系：PDF 标准，原点在左下角，Y 轴向上
- 常见中文 PDF 使用 `1 0 0 -1 e f Tm` 变换矩阵（Y 轴翻转），Vellum 正确处理此情况

### 误差容限

| 参数 | 默认值 | 说明 |
|------|--------|------|
| 同行 Y 容差 | 2 pt | 同一基线上的字形 Y 偏差上限 |
| 词间距阈值 | 1.0 × 字号 | origin-to-origin 间距超过此值则断词 |
| 块间距阈值 | 1.5 × 字号 | 行间距超过此值则分为新块 |

## 依赖

| crate | 用途 |
|-------|------|
| `lopdf 0.40` | PDF 对象级解析与流解压 |
| `memmap2 0.9` | 内存映射文件 |
| `thiserror 2` | 库层结构化错误类型 |
| `anyhow 1` | CLI 层错误传播 |
| `clap 4` | 命令行参数解析 |

## License

[MIT](LICENSE)
