# LogLens

[English](README.md) | 简体中文

> 将一次失败运行转换成适合开发者和 AI 使用的、可追溯的工程证据包。

LogLens 是一个本地优先、模型无关的工程诊断上下文工具。它包装执行测试或程序命令，
采集运行输出和指定日志，将大量非结构化信息整理成结构化事件、重复模式和可读摘要。

LogLens 当前不会调用大模型，也不会修改代码。它首先解决更基础的问题：让后续的人或
AI 获得完整、精简并且可以回到原始现场的证据。

> [!IMPORTANT]
> 项目目前处于早期开发阶段，证据格式和命令行参数在 `1.0` 之前可能调整。

## 为什么需要 LogLens

一次测试或程序运行失败后，开发者面对的往往是：

- 成千上万行 stdout、stderr 和应用日志；
- 重复出现但参数不同的同类错误；
- 测试输出、程序日志和代码版本彼此分离；
- 日志中可能包含 Token、密码等敏感信息；
- AI 能阅读代码，却不知道失败运行时究竟发生了什么；
- AI 给出的结论无法追溯到具体日志证据。

LogLens 不直接替代 CTest、spdlog、日志平台或 AI。它位于这些工具之间：

```text
CTest / 可执行程序
├── stdout / stderr
├── spdlog 文件
├── Git 版本和工作区状态
└── 退出状态
          │
          ▼
       LogLens
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

## 第一阶段已经实现

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
| CI 兼容 | LogLens 返回被包装命令的退出码，不会掩盖失败 |

这些能力不依赖 AI，即使只用于压缩日志和整理失败现场也可以独立工作。

## 快速开始

### 环境要求

- Rust 1.85 或更新版本（Edition 2024）
- Linux、macOS，或其他能够编译当前依赖的平台
- Git 可选；不在 Git 仓库中也可以运行

### 构建

```bash
git clone <your-loglens-repository>
cd loglens
cargo build --release
```

生成的二进制位于：

```text
target/release/loglens
```

### 捕获一次 CTest 运行

假设 C++ 程序使用 spdlog 写入 `build/logs/application.log`：

```bash
touch build/logs/application.log

./target/release/loglens run \
  --log build/logs/application.log \
  --output .loglens/runs \
  -- \
  ctest --test-dir build --output-on-failure
```

`--` 后面的所有内容都是需要执行的原始命令。

如果 CTest 返回 `8`，LogLens 完成诊断包后也会返回 `8`，因此可以直接用于 CI。

### 捕获普通程序

```bash
./target/release/loglens run \
  --log ./logs/service.log \
  -- \
  ./build/bin/service --config ./config/test.yaml
```

即使不提供 `--log`，LogLens 仍会采集 stdout、stderr、退出状态和 Git 信息：

```bash
./target/release/loglens run -- ./build/bin/unit_tests
```

### 捕获多个日志文件

```bash
./target/release/loglens run \
  --log ./logs/parser.log \
  --log ./logs/statistics.log \
  --log ./logs/error.log \
  -- \
  ctest --test-dir build --output-on-failure
```

## 诊断包

每次运行会在输出目录创建一个独立目录：

```text
.loglens/runs/
└── run_<UTC时间>_<进程ID>/
    ├── manifest.json
    ├── summary.md
    ├── events.jsonl
    ├── patterns.json
    ├── stdout.log
    ├── stderr.log
    └── logs/
        ├── 000-application.log
        └── 001-error.log
```

### `manifest.json`

记录本次运行的确定性元数据：

- 命令和参数；
- 开始、结束时间；
- 退出码；
- 工作目录；
- Git commit、分支和工作区状态；
- 日志采集前后的文件大小；
- 生成的证据文件。

### `events.jsonl`

每行是一个结构化事件：

```json
{
  "event_id": "evt_3adf68923eead391",
  "timestamp": "2026-07-30T02:00:01Z",
  "severity": "error",
  "source": "/var/log/application.log",
  "thread_id": "17",
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

提供适合开发者直接阅读的运行摘要，包括：

- 成功或失败状态；
- 原始命令和退出码；
- Git 现场；
- 高信号事件模式；
- 代表性证据 ID；
- 证据追溯说明。

## spdlog 格式

LogLens 当前使用启发式解析，支持常见的 spdlog 风格：

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

未来会增加显式解析配置和不同项目的日志格式 profile。

## 安全与隐私

诊断包默认进行基础脱敏，目前覆盖：

- `Authorization: Bearer ...`
- JWT
- `api_key`
- `access_token` / `auth_token`
- `password` / `passwd`
- `secret`

在完全可信的本地环境中，可以关闭脱敏：

```bash
loglens run --no-redact -- ./build/bin/unit_tests
```

关闭前请确认诊断包不会上传或分享给不可信的系统。

原始日志仍保留在原位置。事件中的字节偏移指向原始文件；如果脱敏改变了文本长度，
该偏移不用于定位诊断包内的脱敏副本。

## 设计原则

### 本地优先

第一阶段不需要服务端、数据库、账号或云平台。

### 模型无关

证据包可以交给任意 AI、IDE、CI 或自研分析工具，核心能力不依赖某个模型厂商。

### 事实与推断分离

当前版本只采集和整理事实，不声称已经找到根因。

### 派生信息不替代原始证据

事件模式和 Markdown 摘要都是派生结果。每个关键结果都应能够回到原始事件。

### 低侵入

第一阶段通过外部命令包装和日志文件读取工作，不要求修改 C++ 业务代码或替换
spdlog。

## 当前限制

- 只采集命令运行期间追加到文件的字节；
- 文件变小会被视为截断或轮转，并从当前文件开头读取；
- 暂不自动发现被重命名的轮转文件；
- 暂无 CTest/JUnit 专用结构化解析器；
- 暂无项目自定义日志格式配置；
- 暂不解析 core dump、ASan、UBSan 或 TSan 专用字段；
- 不复制完整源码或 Git diff；
- 不调用 AI，不生成根因结论，也不修改代码；
- 当前采集和聚合主要在内存完成，不适合无边界的超大日志输入。

## 路线图

### 阶段一：本地证据包（当前）

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

- [ ] CTest 和 GoogleTest 结构化结果
- [ ] 可配置 spdlog profile
- [ ] ASan、UBSan、TSan 输出解析
- [ ] GDB/LLDB 与 core dump 元数据
- [ ] `run_id`、`test_id`、`batch_id` 等上下文关联
- [ ] 更可靠的日志轮转跟踪

### 阶段三：AI 中转层

- [ ] 面向 token 预算的证据选择
- [ ] 明确区分事实、推断和缺失信息
- [ ] AI 使用的稳定诊断上下文协议
- [ ] 本地模型和 OpenAI-compatible 接口
- [ ] MCP 或其他工具调用接口
- [ ] 分析结论到 `event_id` 的强制引用

### 阶段四：通用生态

- [ ] JUnit、pytest、cargo test 等适配器
- [ ] CI 集成
- [ ] 外场日志离线导入
- [ ] OpenTelemetry 兼容
- [ ] 可插拔输入、解析器和输出端

路线图表达方向，不代表兼容性或发布时间承诺。

## 开发

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

仓库包含一个模拟失败运行：

```bash
mkdir -p /tmp/loglens-demo

./target/release/loglens run \
  --log /tmp/loglens-demo/application.log \
  --output /tmp/loglens-demo/runs \
  -- \
  sh examples/demo_failure.sh /tmp/loglens-demo/application.log
```

示例命令会返回非零退出码，这是为了验证 LogLens 不会掩盖测试失败。

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

LogLens 使用 [Apache License 2.0](LICENSE)。
