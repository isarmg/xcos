# xcos 工作流程与流程树

## 1. 总流程树

管理 Web 将首次载入、事件通知、筛选变化、手动刷新和定时刷新交给同一个快照队列。
同一批请求全部结束后才开始下一批；某个接口失败时仍等待其余请求结束。刷新期间收到多个触发时，
队列合并为下一批请求。日志页按服务器日期范围展示已确认与未确认事件；年月日数字直接编辑、回车应用，范围包含起止当天，前后端拒绝非法日期和倒置范围，三类日志均在列表下方提供首页、上一页和下一页。
队列回归使用 `npm run test:unit --prefix web`，页面交互使用 `npm run test:browser --prefix web` 验证。

```text
xcos 1.0.0
├─ 构建
│  ├─ Node 26.7.0 -> check:xcss -> Rust / xcss-web WASM -> wasm-bindgen -> Vite 8
│  ├─ Rust 1.99.0 -> x86_64-unknown-linux-gnu binary
│  └─ Web、Rust、MediaMTX、配置 -> 不可变 release manifest
├─ 启动
│  ├─ 验证当前配置、状态和 MediaMTX contract
│  ├─ 解析当前配置与 Secret
│  ├─ 取得 data_dir单实例锁，systemd 跟踪 companion
│  ├─ 验证当前 SQLite、Schema 与全部实例授权密文
│  ├─ 启动 Rust API、reconciler 与 MediaMTX
│  ├─ 控制面默认 loopback；生产可显式配置内网回源地址，本机回调与 MediaMTX API/playback 仍保留 loopback
│  └─ 已有 HTTPS 网关回源同一项目主机；RTSPS 和 ICE 分别配置
├─ Administrator 控制面
│  ├─ login/session/logout -> Session + CSRF
│  ├─ 授权实例/Client 设备快照 -> 摄像机期望态
│  ├─ reconciler -> MediaMTX actual state
│  └─ 实例、事件、审计与 operation status
├─ 媒体面
│  ├─ 浏览器读取与 Client 发布分别申请资源/动作限定的短时 JWT
│  ├─ MediaMTX internal auth 回调
│  └─ WHEP 实时预览 / HLS 回放
└─ 运维
   ├─ systemctl控制/status/check
   └─ lifecycle + relocated smoke tests
```

产品运行时只理解当前合同，非当前输入明确拒绝。

## 2. 原生发布与首次启动

`scripts/package-release.sh` 只接受干净且 当前版本 annotated tag 指向 HEAD 的 checkout、Linux x86_64 builder，以及与
`config/mediamtx.lock` 匹配的 `linux_amd64` MediaMTX `v1.20.0`。Rust target 固定为
`x86_64-unknown-linux-gnu`，没有其他架构、OS 或 libc 的正式构建分支。脚本构建 Web 和 source-bound
Rust binary，生成完整 manifest，在同一文件系统暂存并验证后，以 no-clobber 语义生成显式指定的 staging 目录，不启动服务。

按[安装指南](getting-started.md)建立普通 `isarmg-xcos` UID、私有 JSON 和独立状态目录，以同一 UID
从 stdin 执行 `init --username admin`，再执行离线 `config validate`。这些步骤不依赖 `/run`。
生产只由归档内的 systemd units 管理；server 不提供进程管理命令。
网关软件由现有基础设施管理，同源媒体、HTTPS、RTSPS TLS 和 ICE 要求见[配置参考](configuration.md)。

## 3. 正式进程启动顺序

1. systemd 为两个服务创建共享的私有 runtime 目录。
2. companion 从应用 JSON 生成本机 callback/recordings 配置，加载独立 TLS/ICE 环境并以前台方式启动。
3. 应用读取私有 JSON，验证当前数据、MediaMTX 文件和业务凭据。
4. 应用取得 database/runtime instance 锁，启动 HTTP 与后台任务。正常启动不扫描发行全树。
5. 检查两个 systemd unit 与实际 readiness；普通启动不会自动初始化。

## 4. Administrator 认证流程

```text
POST /api/v1/auth/login  {username,password}
  -> candidate 为 1..64 bytes printable ASCII
  -> trim ASCII whitespace + ASCII lowercase
  -> canonical 3..64 bytes、首尾字母数字、字符仅 [a-z0-9._-]
  -> 请求体与全局计算/会话容量准入
  -> Argon2 校验
  -> 写 _common_admin_sessions 的 Session/CSRF digest
  -> Set-Cookie: __Host-admin-xcos-session（Secure/HttpOnly/SameSite=Strict）
  -> 返回严格 AdministratorSession

GET /api/v1/auth/session
  -> 验证 Session Cookie 与 idle/absolute TTL
  -> 轮换 CSRF token digest
  -> 返回严格 AdministratorSession

POST /api/v1/auth/logout + X-CSRF-Token
  -> 撤销 Session
  -> 过期 Session Cookie
```

`AdministratorSession` 的 wire 形状固定为
`{authenticated:true,user_id,username,role:"admin",csrf_token}`。`role` 是跨项目 wire 常量，不是数据库字段；
`_common_administrators` 表不保存身份等级，也不存在运行时身份切换。浏览器管理路由要求有效
Administrator Session；unsafe method 还要求当前 CSRF、Origin/Host/URI authority 边界以及单值
`Sec-Fetch-Site: same-origin`。`/api/v1/client/pair` 使用实例授权码，`/api/v1/client/snapshot` 使用
Client Bearer token，二者均不使用浏览器 Session。

摄像机 RTSP/ONVIF 凭据由 Client 保管，Server 摄像机表不存储 URL、username 或 password。
一个加密授权码只对应一个摄像机实例，与 Administrator Session 始终分离。

管理认证 API 和 xcss-web/WASM Web 属于 Server 控制面；Client 配对、MediaMTX internal auth、
媒体 JWT subject/camera/actions、录像和播放属于数据面。两类身份不得共用存储或日志字段。

## 5. 授权实例与媒体状态机

```text
Administrator 创建授权实例 -> 加密保存授权码
  -> Client 以该码配对，快照最多包含一台摄像机；空快照移除原有摄像机
  -> SQLite transaction: camera snapshot + desired media state + operation
  -> reconciler 领取 global lease + operation lease
  -> transaction 外调用 MediaMTX
       ├─ 成功 -> actual path + succeeded
       ├─ 明确失败 -> failed + 可解释错误
       └─ 结果不可证明 -> unknown
  -> 更换授权码要求 Client 重新配对
  -> 撤销：幂等 revoked + 清理路径；独立 /delete 路由永久删除实例
```

只有仍同时持有未过期全局/操作租约的 owner 能 finalize。启动先取得独占实例锁，再把上一进程遗留的全部
running 标为 unknown，包括 operation lease 尚未过期的记录；全局 lease 保留至到期。运行中的常规租约回收
只处理已过期的 operation，不抢占健康 owner。撤销会停用并隐藏摄像机，重复撤销保留实例；独立永久删除只在 MediaMTX 清理已确认后执行；
录像字节不会因删除页面条目而被隐式擦除。

PTZ 不由 Server 直连 ONVIF：`POST /api/v1/cameras/{id}/ptz` 将有期限命令排队给所属 Client，
Client 在快照通道回报结果。网络结果不确定时不能自动重放 move。

## 6. 漂移修复

周期任务读取期望态，在事务外读取 MediaMTX 配置、Publisher、Recording 实际态；摘要比较发现差异
后创建 `drift_detected` 操作。日志和持久错误仅包含 camera/operation ID 与固定错误码，不包含 RTSP
URL、userinfo、用户名、密码或远端错误正文。

## 7. 播放授权

```text
Administrator Browser Session
  -> /api/v1 请求单摄像头媒体授权
  -> 当前 HKDF key 签发短时 JWT
  -> Browser 访问同源 /media-webrtc 或 /media-hls
  -> Reverse proxy 转给 MediaMTX
  -> MediaMTX 调用 /internal/v1/media/auth
  -> Rust 验证 protocol、issuer、audience、kind、camera、jti 与时间
```

媒体 JWT 只能授权指定资源与用途，不能调用管理 API；Session Cookie 也不能替代媒体 JWT。

## 8. 停机与代际数据边界

`systemctl stop xcos.service` 根据 unit 依赖先停止应用，再停止 MediaMTX，释放数据库/runtime 锁。直接 kill 或乱序停止可能把外部效果留在 unknown，应保全日志后由 reconciler/人工判断。

普通运行不扫描其他代路径、不解析非当前Schema/密文，也不通过fallback修补数据。实例和 runtime 锁核对物理身份，systemd 管理 companion，错误输入拒绝运行。

## 9. Rust Web 构建与验证流程

```text
web/vendor/*/SOURCE.json + web/xcss-web/vendor/xcss-web/SOURCE.json
  -> 校验公共资源和 CSR 源码来源与逐文件摘要
  -> web/xcss-web/Cargo.lock + cargo wasm32-unknown-unknown 检查与测试
  -> xcss-web-core 0.8.19 release + wasm-bindgen 0.2.129
  -> Rust tools/web-build 整理 WASM、样式、字体和按需 HLS
  -> dist/index.html + hashed assets
  -> native build 纳入 release manifest
  -> relocated smoke 验证引用、重定位与篡改拒绝
```

生产构建无需 npm；Node/Playwright 只用于验收。开发与验证命令：

```bash
npm ci --prefix web
npm run check:xcss --prefix web
npm run build --prefix web
npm run test:browser --prefix web
```

`check:xcss` 检查实际 vendor 资源摘要、当前 Rust 认证外壳、启动入口和设计边界，CI 独立执行该门禁。
Rust 构建工具校验来源、锁文件及资源预算，不读取 node_modules；浏览器也不访问 npm registry、xcss 仓库或远程 CSS。
服务端来源以根 Cargo 声明、锁文件与 xcss-product.toml 的完整修订为准；浏览器 CSR 和静态资源分别绑定其 SOURCE.json。
它们不读取共同父目录或 sibling checkout，也不提供旧来源 fallback。输入散列及 Cargo 包校验值进入 `assets/xcss-web-build.json`。

## 10. xcss 共享层与产品层调用树

```text
web/src/main.js
├─ @xcss/web 的 token、字体、reset、accessibility 和轮廓绘制适配
└─ web/pkg/xcos_web.js + xcos_web_bg.wasm
   └─ web/xcss-web/src/lib.rs
      ├─ xcss-web：认证会话、同源 HTTP、CSRF、超时和响应预算
      ├─ 实例、名称草稿、快照队列和事件流
      ├─ logs.rs：UTC 日期与独立游标分页
      ├─ recordings.rs：录像结果身份与播放器生命周期
      ├─ controls.rs：串行 PTZ 与设备回执
      └─ media.rs：WHEP、资源清理、重试和 HLS 回退
         └─ 按需加载 hls.js 浏览器引擎（不承载页面或业务状态）
```

共享层拥有管理员密码策略、Session/CSRF 生命周期、Cookie 构造及公共平台表等跨产品机制；
产品选择自身 ID，组合当前 Schema，并拥有业务 DTO、业务数据库语义、媒体状态机、页面和品牌。
产品层不得复制共享机制形成第二事实源。

xcss-web 构建环境、边界和真实 WASM 验收见 [管理前端维护](xcss-web.md)。
