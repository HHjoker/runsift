# RunSift

[English](README.md) | 简体中文

> 将实时失败或历史日志转换成适合开发者和 AI 使用的、可追溯工程证据。

RunSift 是一个本地优先、模型无关的工程诊断上下文工具。它既能包装执行测试或程序，
也能在问题无法复现时完整导入历史日志。两种入口都会将非结构化信息整理成结构化
事件、高信号模式、可追溯证据和开发者摘要。

`run`、`import` 和 `context` 不会调用大模型，也不会修改源码或原始日志。只有显式
执行 `analyze` 并选择适配器时才会访问模型。这样可以先独立解决更基础的问题：让
后续的人或 AI 获得精简、可靠并且可以回到原始现场的证据。

> [!IMPORTANT]
> 项目目前处于早期开发阶段，证据格式和命令行参数在 `1.0` 之前可能调整。
> Rust library API 目前也不保证稳定。RunSift `0.4` 生成的证据格式版本为 `3`，
> 诊断上下文协议版本为 `2`，并继续读取旧的 schema v2 证据包。

## 为什么需要 RunSift

一次测试或程序运行失败后，开发者面对的往往是：

- 成千上万行 stdout、stderr 和应用日志；
- 重复出现但参数不同的同类错误；
- 测试输出、程序日志和代码版本彼此分离；
- 日志中可能包含 Token、密码等敏感信息；
- AI 能阅读代码，却不知道失败运行时究竟发生了什么；
- AI 给出的结论无法追溯到具体日志证据。

RunSift 不直接替代 CTest、spdlog、日志平台或 AI。它位于这些工具之间：

```text
CTest / 可执行程序
├── stdout / stderr
├── spdlog 文件
├── Git 版本和工作区状态
└── 退出状态
          │
          ▼
       RunSift
├── 增量采集
├── 事件解析
├── 多行日志合并
├── 敏感信息脱敏
├── 动态字段归一化
├── 重复模式聚合
└── 证据引用
          │
          ▼
本地诊断包
├── 人工查看
├── AI 分析
├── CI 后续步骤
└── 其他分析工具
```

## 已实现能力

| 能力 | 当前行为 |
|---|---|
| 命令包装 | 执行任意命令并保留原始退出码 |
| 运行输出 | 实时回显并采集 stdout、stderr |
| 日志增量采集 | 只收集命令运行期间追加到指定文件的内容 |
| spdlog 解析 | 识别常见时间、级别和线程 ID |
| 多行事件 | 合并缩进内容和常见堆栈行 |
| 模式聚合 | 归一化时间、数字、地址、UUID 和 IP 后合并同类事件 |
| 证据追溯 | 每个事件包含稳定 ID、原始路径和字节偏移 |
| 默认脱敏 | 处理 Bearer Token、JWT、API Key、密码和 Secret |
| Git 现场 | 记录仓库、commit、分支和修改文件 |
| 双重输出 | 同时生成机器可读 JSON/JSONL 和人工可读 Markdown |
| CI 兼容 | RunSift 返回被包装命令的退出码，不会掩盖失败 |

这些能力不依赖 AI，即使只用于压缩日志和整理失败现场也可以独立工作。

历史证据导入现在是一等入口：

| 能力 | 当前行为 |
|---|---|
| 完整文件导入 | 不重新运行程序，读取已有日志的全部字节 |
| 文件和目录 | 支持多个路径、目录直接文件，以及使用 `--recursive` 递归导入 |
| 来源完整性 | 记录原始路径、大小、修改时间、SHA-256 和字节级证据位置 |
| 关键信息抽取 | 提取 WARN 以上、Sanitizer，以及没有明确级别的失败相关关键字 |
| 开发者摘要 | 输出来源清单、时间范围、级别统计、关键事件时间线和重复模式 |
| Case 关联 | 使用稳定 `case_id` 组织历史证据，不伪造不存在的命令和退出状态 |
| 原子输出 | 所有请求输入处理成功后才发布最终证据包 |

第二阶段增加了面向 C++ 工程的上下文：

| 能力 | 当前行为 |
|---|---|
| CTest 和 GoogleTest | 将 JUnit XML 导入为稳定的测试用例记录 |
| spdlog profile | 通过具名捕获的 JSON profile 解析项目自定义格式 |
| Sanitizer | 提取 ASan、UBSan、TSan 问题及其调用栈 |
| 崩溃上下文 | 记录 core 元数据并导入 GDB/LLDB 文本报告 |
| 上下文关联 | 将 `run_id`、`batch_id`、`test_id` 传递到证据中 |
| 日志轮转 | 在类 Unix 系统利用文件身份找回 rename 轮转前后的两段日志 |

第三阶段将证据转换为受控的 AI 输入和输出：

| 能力 | 当前行为 |
|---|---|
| Token 预算 | 使用确定性的近似预算，优先选择高价值证据 |
| 上下文协议 | 分开表达事实、推断、缺失信息、证据和响应约束 |
| 证据引用 | 拒绝引用上下文之外证据 ID 的分析结论和推断 |
| 本地适配器 | 通过 stdin 将 prompt 交给显式指定的本地进程 |
| OpenAI-compatible 适配器 | 默认支持 Responses，也提供 Chat Completions 兼容模式 |
| 工具集成 | 将上下文输出为 stdout JSON，供 Agent、CI 或自研工具使用 |

## 快速开始

### 环境要求

- Rust 1.85 或更新版本（Edition 2024）
- Linux、macOS，或其他能够编译当前依赖的平台
- Git 可选；不在 Git 仓库中也可以运行

### 构建

```bash
git clone https://github.com/HHjoker/runsift.git
cd runsift
cargo build --release
```

生成的二进制位于：

```text
target/release/runsift
```

### 导入历史日志

导入一份从外场取回的日志：

```bash
./target/release/runsift import \
  --case-id field-4821 \
  /path/to/application.log
```

原始日志只读。RunSift 在 `.runsift/cases/field-4821/` 下生成新证据包，并在终端提示
`summary.md` 的位置。

导入多个日志或整个目录树：

```bash
./target/release/runsift import \
  --case-id customer-crash-4821 \
  --recursive \
  ./field-logs/ ./gateway.log
```

有相关证据时可以一并附加：

```bash
./target/release/runsift import application.log \
  --test-report gtest-results.xml \
  --debugger-report gdb-backtrace.txt \
  --core core.1234
```

先查看开发者摘要，再按需生成本地 AI 上下文：

```bash
less .runsift/cases/field-4821/summary.md

./target/release/runsift context \
  .runsift/cases/field-4821 \
  --token-budget 8000
```

### 捕获一次 CTest 运行

假设 C++ 程序使用 spdlog 写入 `build/logs/application.log`：

```bash
touch build/logs/application.log

./target/release/runsift run \
  --log build/logs/application.log \
  --output .runsift/runs \
  -- \
  ctest --test-dir build --output-on-failure
```

`--` 后面的所有内容都是需要执行的原始命令。

如果 CTest 返回 `8`，RunSift 完成诊断包后也会返回 `8`，因此可以直接用于 CI。

### 捕获普通程序

```bash
./target/release/runsift run \
  --log ./logs/service.log \
  -- \
  ./build/bin/service --config ./config/test.yaml
```

即使不提供 `--log`，RunSift 仍会采集 stdout、stderr、退出状态和 Git 信息：

```bash
./target/release/runsift run -- ./build/bin/unit_tests
```

### 捕获多个日志文件

```bash
./target/release/runsift run \
  --log ./logs/parser.log \
  --log ./logs/statistics.log \
  --log ./logs/error.log \
  -- \
  ctest --test-dir build --output-on-failure
```

### 采集结构化 C++ 测试和运行证据

CTest 可以在 RunSift 采集本次运行时同步生成 JUnit XML：

```bash
./target/release/runsift run \
  --test-report build/ctest-results.xml \
  --batch-id ci-4821 \
  -- \
  ctest --test-dir build \
    --output-on-failure \
    --output-junit build/ctest-results.xml
```

GoogleTest 使用其 XML 输出：

```bash
./target/release/runsift run \
  --test-report build/gtest-results.xml \
  --test-id parser-suite \
  -- \
  ./build/parser_tests --gtest_output=xml:build/gtest-results.xml
```

写入 stderr 的 ASan、UBSan、TSan 报告会被自动识别。已有的 core dump 和调试器
报告可以通过 `--core` 与 `--debugger-report` 附加：

```bash
runsift run \
  --core build/core.1234 \
  --debugger-report build/lldb-backtrace.txt \
  -- \
  ./build/parser_tests
```

RunSift 只记录 core 元数据，不复制可能非常大的 core 文件，也不会主动执行调试器。
传入的 GDB/LLDB 文本报告会经过脱敏后复制到诊断包中。

### 在本地生成 AI 上下文

`run` 或 `import` 创建证据包后，可以按近似 Token 预算选择其中信号最高的证据：

```bash
runsift context .runsift/runs/<run_id> --token-budget 8000
```

历史问题使用 `.runsift/cases/<case_id>`。

该命令写入 `ai/context.json` 和 `ai/prompt.md`，只在本地工作，不访问模型。
如果其他工具需要机器可读输出：

```bash
runsift context .runsift/runs/<run_id> --stdout
```

上下文包含已观察事实、刻意保持为空的推断列表、已知证据缺口、选中的证据和精确的
响应格式。详见[诊断上下文协议](docs/diagnostic-context-v2.md)。

### 通过显式适配器进行分析

任意能够从 stdin 读取 prompt、在 stdout 返回 RunSift 分析 JSON 的本地命令都可接入：

```bash
runsift analyze .runsift/runs/<run_id> local -- \
  ollama run qwen3
```

也可以调用 OpenAI-compatible 服务：

```bash
export OPENAI_API_KEY="..."

runsift analyze .runsift/runs/<run_id> openai \
  --model <MODEL> \
  --api-key-env OPENAI_API_KEY
```

默认使用 Responses API 结构。只实现了旧兼容端点的服务可以添加
`--api chat-completions`。RunSift 会校验返回 JSON；任何没有引用已选证据的结论或
推断都不会被写入有效分析文件。

## 诊断包

实时运行和历史问题使用不同的父目录，但下游采用相同证据接口：

```text
.runsift/runs/
└── run_<UTC时间>_<进程ID>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── tests.json
    ├── diagnostics.json
    ├── crash.json
    ├── stdout.log
    ├── stderr.log
    ├── ai/
    │   ├── context.json
    │   ├── prompt.md
    │   └── analysis.json
    ├── debugger/
    │   └── 000-lldb-backtrace.txt
    ├── tests/
    │   └── 000-ctest-results.xml
    └── logs/
        ├── 000-application.log
        └── 001-error.log

.runsift/cases/
└── case_<UTC时间>_<进程ID>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── tests.json
    ├── diagnostics.json
    ├── crash.json
    └── logs/
        ├── 000-application.log
        └── 001-application.log.1
```

### `manifest.json`

记录本次采集或导入的确定性元数据：

- `capture_mode`（`live` 或 `import`）；
- 实时证据使用 `run_id`，历史证据使用 `case_id`；
- 实时采集存在时记录命令、参数和退出码；
- 处理时间范围和日志中识别到的时间范围；
- 工作目录；
- 实时采集可选的 Git commit、分支和工作区状态；
- 原始来源路径、大小、修改时间、SHA-256 和复制产物；
- 日志采集前后的文件大小；
- 结构化测试、Sanitizer、core 和调试器计数；
- 生成的证据文件。

### `events.jsonl`

每行是一个结构化事件：

```json
{
  "event_id": "evt_3adf68923eead391",
  "context": {
    "run_id": "run_ci_4821",
    "batch_id": "ci_4821",
    "test_id": "parser_suite"
  },
  "timestamp": "2026-07-30T02:00:01Z",
  "severity": "error",
  "source": "/var/log/application.log",
  "thread_id": "17",
  "logger": "parser",
  "message": "invalid record length 18 at offset 8192",
  "evidence": {
    "artifact": "logs/000-application.log",
    "source_path": "/var/log/application.log",
    "byte_start": 420,
    "byte_end": 527
  }
}
```

后续分析结论应引用 `event_id`，而不是只输出一段无法验证的自然语言。

### `patterns.json`

将动态参数不同但结构相同的事件归为同一模式：

```text
invalid record length 18 at offset 8192
invalid record length 21 at offset 17664
```

归一化为：

```text
invalid record length <num> at offset <num>
```

模式中保留出现次数、严重级别、首次和最后观察时间，以及代表事件 ID。

### `summary.md`

提供适合开发者直接阅读的摘要，包括：

- 证据类型和来源清单；
- 存在可靠时间戳时的历史日志时间范围；
- 日志级别统计和高信号事件时间线；
- 实时采集存在时的命令、退出状态和 Git 现场；
- 高信号事件模式；
- 代表性证据 ID；
- 证据追溯说明。

### C++ 专用证据

- `tests.json` 保存 CTest/GoogleTest 用例、状态、耗时、失败信息、稳定的 `test_id`，
  并指向复制到 `tests/` 下的脱敏源 XML；
- `diagnostics.json` 保存结构化 ASan、UBSan、TSan 问题、调用栈和字节级证据位置；
- `crash.json` 保存轻量 core dump 元数据和解析后的 GDB/LLDB 报告。
- `ai/context.json` 和 `ai/prompt.md` 保存本地生成、受预算约束的模型交接内容；
  `ai/analysis.json` 只在显式调用适配器并通过引用校验后生成。

## spdlog 格式

RunSift 当前使用启发式解析，支持常见的 spdlog 风格：

```text
[2026-07-30T10:00:00+08:00] [error] [thread 17] parse failed
  at parser.cpp:42
```

建议日志至少包含：

```text
时间 | 级别 | 线程 ID | 模块或 logger | 消息
```

带显式时区的时间会统一转换为 UTC。没有时区的时间仍保留在原始消息中，但当前不会
猜测它属于哪个时区。

项目自定义格式可以通过 JSON profile 配置：

```bash
runsift run \
  --log build/logs/application.log \
  --log-profile examples/spdlog-profile.json \
  -- \
  ./build/parser_tests
```

正则表达式必须定义具名的 `level` 和 `message` 捕获组，还可以定义 `timestamp`、
`thread` 和 `logger`。`timestamp_format` 使用 chrono 时间格式；当日志时间本身不含
时区时，profile 必须提供 `timezone`。参考
[`examples/spdlog-profile.json`](examples/spdlog-profile.json)。

同一个 profile 也能用于历史日志：

```bash
runsift import --log-profile examples/spdlog-profile.json ./field-logs
```

## 安全与隐私

诊断包默认进行基础脱敏，目前覆盖：

- `Authorization: Bearer ...`
- JWT
- `api_key`
- `access_token` / `auth_token`
- `password` / `passwd`
- `secret`

命令行参数也使用相同规则脱敏；生成 AI 上下文时还会再次执行脱敏，包括读取旧诊断包
的场景。

在完全可信的本地环境中，可以关闭脱敏：

```bash
runsift run --no-redact -- ./build/bin/unit_tests
```

关闭前请确认诊断包不会上传或分享给不可信的系统。

`run`、`import` 和 `context` 不上传证据。`import` 不修改来源文件，并在写入脱敏副本
前计算原始字节 SHA-256。`analyze local` 只调用用户指定的进程；`analyze openai` 会把
生成的 prompt 发送到配置的 base URL。API Key 从指定环境变量读取，不会写入诊断包。

原始日志仍保留在原位置。事件中的字节偏移指向原始文件；如果脱敏改变了文本长度，
该偏移不用于定位诊断包内的脱敏副本。

## 设计原则

### 本地优先

第一阶段不需要服务端、数据库、账号或云平台。

### 模型无关

证据包可以交给任意 AI、IDE、CI 或自研分析工具，核心能力不依赖某个模型厂商。

### 事实与推断分离

RunSift 生成的上下文将已观察事实与模型推断分开，并显式说明缺失信息。模型可以提出
根因，但所有结论必须引用已选证据。

### 派生信息不替代原始证据

事件模式和 Markdown 摘要都是派生结果。每个关键结果都应能够回到原始事件。

### 低侵入

第一阶段通过外部命令包装和日志文件读取工作，不要求修改 C++ 业务代码或替换
spdlog。

## 当前限制

- `run` 只采集命令运行期间追加的字节；已有完整文件请使用 `import`；
- 历史导入目前接受普通文件和目录，尚不自动展开 `.gz`、`.zip`、`.tar.gz`；
- 来源文件在读取期间必须保持稳定；如果大小或修改时间发生变化，RunSift 会拒绝本次
  导入；
- 导入文本当前在内存中处理，暂不适合无边界或数 GB 级日志集合；
- 没有显式时区时间戳时，RunSift 保留来源和字节顺序，但不声称建立了可靠的跨文件
  时间线；
- 轮转日志可以作为多个文件导入，但记录无时间戳时暂不根据文件名推断并重排顺序；
- 文件变小会被视为截断或轮转，并从当前文件开头读取；
- rename 方式的轮转恢复在类 Unix 系统依赖文件身份，只扫描被监控日志所在的直接目录；
- copy-truncate 轮转可以被发现，但结束采集前已经写入又被删除的字节无法找回；
- CTest 当前读取 JUnit XML，不读取旧式 `Testing/*/Test.xml`；
- RunSift 不主动执行 GDB/LLDB，只导入预先生成的文本报告；
- core 文件只记录元数据，不复制到诊断包；
- 不复制完整源码或 Git diff；
- Token 数量是与厂商无关的确定性估算，不是具体模型 tokenizer 的精确结果；
- OpenAI 适配器保持精简，暂不支持重试、流式响应、对话状态或厂商特定工具调用；
- 已提供 JSON stdout 工具接口，尚未实现独立 MCP Server；
- RunSift 能校验证据 ID 和响应格式，但不能证明模型解释在逻辑上一定正确；
- RunSift 不会修改源码；
- 当前采集和聚合主要在内存完成，不适合无边界的超大日志输入。

## 路线图

### 阶段一：本地证据包

- [x] 包装任意命令
- [x] 捕获 stdout、stderr 和退出码
- [x] 增量采集指定日志
- [x] 常见 spdlog 解析
- [x] 多行事件
- [x] 模式聚合
- [x] 基础脱敏
- [x] Git 元数据
- [x] JSONL/JSON/Markdown 诊断包

### 阶段二：C++ 工程增强

- [x] CTest 和 GoogleTest 结构化结果
- [x] 可配置 spdlog profile
- [x] ASan、UBSan、TSan 输出解析
- [x] GDB/LLDB 与 core dump 元数据
- [x] `run_id`、`test_id`、`batch_id` 等上下文关联
- [x] 更可靠的日志轮转跟踪

### 阶段三：AI 中转层（当前）

- [x] 面向 token 预算的证据选择
- [x] 明确区分事实、推断和缺失信息
- [x] AI 使用的稳定诊断上下文协议
- [x] 本地模型和 OpenAI-compatible 接口
- [x] 机器可读 CLI 工具接口（`context --stdout`）
- [x] 分析结论到证据 ID 的强制引用
- [ ] 独立 MCP Server

### 阶段四：历史问题证据（当前）

- [x] 完整历史日志文件导入
- [x] 多文件和递归目录输入
- [x] 来源 SHA-256、元数据、产物和字节偏移追溯
- [x] 历史时间范围和高信号开发者摘要
- [x] `context` 和 `analyze` 支持历史证据
- [x] 可选测试、调试器和 core 证据
- [ ] 压缩包输入
- [ ] 显式问题锚点和时间窗口
- [ ] 无时间戳轮转文件族推断

### 阶段五：通用生态

- [ ] JUnit、pytest、cargo test 等适配器
- [ ] CI 集成
- [ ] OpenTelemetry 兼容
- [ ] 可插拔输入、解析器和输出端

路线图表达方向，不代表兼容性或发布时间承诺。

## 开发

项目将生产代码和测试代码分开存放：

```text
src/
├── lib.rs          # 可复用的采集和解析库
├── main.rs         # 精简的 CLI 入口
└── *.rs            # 生产代码模块
tests/
├── support/        # 测试公共工具
└── *.rs            # 黑盒集成测试
```

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

仓库包含一个模拟失败运行：

```bash
mkdir -p /tmp/runsift-demo

./target/release/runsift run \
  --log /tmp/runsift-demo/application.log \
  --output /tmp/runsift-demo/runs \
  -- \
  sh examples/demo_failure.sh /tmp/runsift-demo/application.log
```

示例命令会返回非零退出码，这是为了验证 RunSift 不会掩盖测试失败。

不运行任何程序也可以体验历史导入：

```bash
./target/release/runsift import \
  --case-id historical-demo \
  --output /tmp/runsift-cases \
  examples/historical_failure.log

less /tmp/runsift-cases/historical-demo/summary.md
```

第二阶段还提供了包含 GoogleTest XML、自定义 spdlog profile 和模拟 ASan 报告的
C++ 上下文示例：

```bash
mkdir -p /tmp/runsift-phase2
touch /tmp/runsift-phase2/application.log

./target/release/runsift run \
  --log /tmp/runsift-phase2/application.log \
  --log-profile examples/spdlog-profile.json \
  --test-report /tmp/runsift-phase2/gtest.xml \
  --output /tmp/runsift-phase2/runs \
  --batch-id demo-batch \
  --test-id parser-suite \
  -- \
  sh examples/demo_phase2_failure.sh \
    /tmp/runsift-phase2/application.log \
    /tmp/runsift-phase2/gtest.xml
```

可以直接使用生成的目录测试第三阶段，并且不访问任何模型：

```bash
runsift context /tmp/runsift-phase2/runs/<run_id> --token-budget 8000
```

## 参与贡献

项目仍在早期阶段，以下贡献尤其有价值：

- 来自真实工程的匿名化失败日志；
- CTest、GoogleTest 和不同 spdlog 格式样例；
- 日志轮转、崩溃和并发输出的边界案例；
- 证据格式和隐私策略建议；
- 不同平台的构建与测试反馈；
- 能够验证“压缩后仍保留关键根因证据”的测试案例。

提交问题时请先删除日志中的敏感数据，并尽量附上：

1. 执行命令；
2. 预期行为；
3. 实际行为；
4. 最小日志样例；
5. 操作系统和 Rust 版本。

## License

RunSift 使用 [Apache License 2.0](LICENSE)。
