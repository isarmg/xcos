# 运维

server 只有 `bin/xcos --root ROOT` 运行入口。安装、离线维护由 xssi 脚本和独立 `libexec/setup` 完成；升级由包内升级脚本调用独立 XSSC。程序不配置或探测代理，不识别访客 IP。

```sh
sudo systemctl status xcos.service xcos-mediamtx.service
sudo /opt/isarmg/xcos/maintenance.sh --root /opt/isarmg/xcos check
curl --fail http://127.0.0.1:18106/readyz
sudo /opt/isarmg/xcos/upgrade.sh --root /opt/isarmg/xcos --version VERSION --xssc /absolute/xssc
```

升级自动下载并SHA-256 校验完整包，停止 Xcos 和 MediaMTX、等待实际进程退出、取得维护锁、备份并按目标当前结构只读检查、替换SHA-256 文件清单中的程序文件，再启动并核对实际版本和真实就绪。失败或进程中断使用持久事务恢复；不能通过删锁、重建数据库或重新生成密钥绕过检查。仅支持当前数据合同，不导入旧布局或旧配置。

管理员改密可以通过管理后台完成；离线重置使用维护脚本，密码经标准输入：

```sh
sudo systemctl stop xcos.service xcos-mediamtx.service
systemd-ask-password 'New administrator password:' | sudo /opt/isarmg/xcos/maintenance.sh \
  --root /opt/isarmg/xcos admin-reset --username admin
sudo systemctl start xcos.service
```

重置保留管理员稳定 ID 和实例密钥，递增会话版本并撤销原会话。密钥缺失或权限错误直接阻止运行，绝不补建。

应用 UTC 结构日志写入 `ROOT/logs`，临时运行文件在 `ROOT/run`；数据库、secrets.json 和公共锁默认在 `ROOT/state`。MediaMTX 生成配置在 `ROOT/config/mediamtx.yml`，模板为 `ROOT/share/mediamtx.yml`。journal 可用于查看应用和媒体标准流。卸载由用户自行停止服务、移除 systemd 和程序；业务数据保留或删除由用户决定。

真实 RTSPS TLS、NAT/ICE 和实际摄像头链路需要在目标环境验证。[配置说明](configuration.md)、[故障排查](troubleshooting.md)。
