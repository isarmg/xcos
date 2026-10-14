# 09. 部署、安全与生产运维

## 9.1 生产布局

只读程序、0600 JSON、SQLite、runtime locks、MediaMTX config/contract 和 recordings 分区管理。
服务账户只能写业务状态，不得修改 binary、Web 或 manifest。Axum 默认/正式样例均监听
`127.0.0.1:18106`；公网只暴露已有可信 HTTPS 网关和必要媒体入口。

## 9.2 上线流程

验证 tag/归档/checksum；安装已验证的完整程序 payload；设置 ownership/mode；准备当前组合状态和 external key；运行
显式 xssi SHA-256 校验包校验 与 离线check；确认已有 HTTPS 网关、RTSPS TLS 和 ICE 路由；通过 systemd 启动
MediaMTX 和 Xcos；检查两个 readiness、登录、摄像头读取和实验播放。任一步失败停止，不覆盖原目录。

## 9.3 配置和 Secret

管理员密码、credential key、JWT/signing material、摄像头密码与 TLS key 分开管理。私有 JSON 和 companion 环境文件均为 0600，父目录 0700。轮换要有变更窗口、备份、验证与明确回滚点。

## 9.4 监控指标

关注 readiness、登录拒绝、Session、operation backlog/age/unknown、lease、reconcile latency/error、
MediaMTX API、publisher、播放错误、录像磁盘/inode、SQLite/WAL、审计写入 warning 和 SSE resync。

## 9.5 日常 离线 check

离线check 验证 Schema/SQLite/key/MediaMTX 合同与录像目录；online 再探测运行进程。check
失败是停止自动变更的信号，不是手改 metadata 或跳过检查的理由。

## 9.7 事件处置

疑似入侵时隔离公网/摄像头 VLAN，停止扩大写入，保全 release SHA、数据库 generation、录像、配置摘要、
审计和日志；轮换 Session、管理员密码、JWT、credential key、摄像头密码和 TLS 材料。

## 9.8 故障表

| 故障 | 操作 |
|---|---|
| operation unknown | 冻结同资源写入，核对实际状态，禁止盲重试 |
| key 解密失败 | 检查受保护 key 来源/ID；产品只接受当前唯一 key |
| Schema drift | 停止并保全 generation 和错误记录 |
| 录像盘满 | 停止新增写入，按策略扩容/归档，不手删控制文件 |
| MediaMTX 合同不符 | 恢复精确制品/config，不临时放宽 SHA |
