# 02. 开发环境与第一次运行

## 2.1 所需工具

生产 Web 构建需要 Rust 1.99.0、wasm32-unknown-unknown target 和 wasm-bindgen-cli 0.2.129，无需 npm 安装。浏览器验收另需 Node 26.7.0 和 Web 锁文件中的开发依赖。完整原生生命周期还需要 Linux、shell 工具以及
仓库固定的 MediaMTX 制品。不要用系统中碰巧安装的另一个 MediaMTX 代替合同资产。

```bash
cargo +1.99.0 run --quiet --locked --manifest-path tools/web-build/Cargo.toml -- server --product-root . --config xcss-web-build.json --mode development
cargo +1.99.0 check --locked --all-targets
```

## 2.2 临时实验环境

按[安装指南](../getting-started.md)在新的绝对临时 ROOT 执行已校验发行包的 xssi 安装流程，使用 development=true 的私有配置和实验 TLS 材料。安装流程负责当前密钥、数据库、管理员及 MediaMTX 配置初始化，密码经标准输入读取；Server 本身没有初始化子命令。安装成功后保持服务停止，避免实验进程与 systemd 同时写入。摄像头的 RTSP/ONVIF 地址、用户名和密码只保存在 Client，Server 摄像头表没有对应凭据列。

## 2.3 第一次开发启动

开发模式只绑定回环，未提供 `XCOS_BIND` 时生产解析器也安全默认到 `127.0.0.1:18106`。使用上方统一构建
得到的内嵌 Web binary 和已初始化临时 ROOT，以安装所用服务账号执行 `--root`。配置来源为 ROOT/config/config.json 及当前声明的环境映射，样例见 `config/xcos.json.example`。生产对外访问由可信 HTTPS 网关转发。

```bash
sudo -u isarmg-xcos /absolute/xcos-debug --root /absolute/development-root
```

仅开发目录热更新时设置 `XCOS_STATIC_DIR="$PWD/web/dist"`；重新构建 Web 后无需重编 Rust。
生产模式拒绝此覆盖。

## 2.4 启动 companion

正式部署使用 `systemctl start xcos.service`，由 systemd 以同一普通 UID 启动 MediaMTX 和 xcos。开发进程使用独立临时目录。自动化媒体与生命周期实验使用[开发指南](../development.md)中的脚本，再分别检查应用和 companion readiness。

## 2.5 第一个摄像机实例练习

只使用实验摄像头或本地测试源：管理员在菜单中创建实例，把授权码配置到 Client，
由 Client 选择一台摄像机并上报，确认 MediaMTX publisher/path，再通过可信代理测试播放。
设备密码不应离开 Client，也不应出现在浏览器状态、审计或日志中。

## 2.6 成功标准

- release/config/Schema/credential 预检均通过；
- 两个进程各自只存在一个实例；
- 创建操作持久化并到达可解释终态；
- 浏览器只拿短期播放授权；
- 重启取得独占实例锁后，pending 可继续，上一进程遗留的所有 running 转为 unknown，即使 operation lease 尚未到期；全局 lease 保留，运行期不抢占健康 owner；
- 停服后的维护脚本 check 与运行时真实 readiness 检查分别通过。

## 2.7 常见失败

| 现象 | 先检查 |
|---|---|
| MediaMTX 拒绝启动 | binary SHA、版本、config exact contract |
| Xcos 拒绝库 | metadata、DDL SHA、lease、密文或锁 |
| 登录循环 | HTTPS、Secure Cookie、Origin/Host、系统时钟 |
| 登录 400 | 请求是否精确为 `username/password`，username 是否符合 3–64 bytes canonical 规则 |
| operation 不结束 | reconciler lease、MediaMTX API、网络 |
| 无画面 | RTSP source、publisher、JWT、代理 WHEP/HLS 路由 |

## 2.8 实验清理

停止两个进程并确认锁释放后删除临时根。不要把临时 credential key、数据库或摄像头 URL 提交到仓库，
也不要拿生产录像做测试 fixture。
