# 运维

正式环境只用systemd。xcos和MediaMTX都以普通isarmg-xcos UID前台运行。xcosctl只是systemctl薄封装。

```sh
sudo systemctl status xcos.service xcos-mediamtx.service
sudo systemctl restart xcos.service
sudo -u isarmg-xcos /opt/isarmg/xcos/bin/xcos config validate
sudo -u isarmg-xcos /opt/isarmg/xcos/bin/xcos check
sudo -u isarmg-xcos /opt/isarmg/xcos/bin/xcos status
```

config validate只检查配置，不要求数据库、密钥、录像目录已创建；media-config也可在初始化前生成。
check是离线实例检查，覆盖schema、账户、密钥/凭据解密、录像存储和兼容MediaMTX；status查询真实就绪。
所有命令使用全局--config与--data-dir；不再提供doctor别名。

管理员密码维护须停服，密码从stdin读取：

```bash
sudo systemctl stop xcos.service
read -rsp 'New administrator password: ' password; echo
printf '%s\n' "$password" | sudo -u isarmg-xcos /opt/isarmg/xcos/bin/xcos admin reset-password --username admin
unset password
sudo systemctl start xcos.service
```

密码修改与session撤销由公共认证事务完成，不更换JWT/凭据密钥。
应用UTC结构日志由公共层轮转到data_dir/logs，log_level默认info；journal收集应用和媒体标准流。
不自行管理app.log/mediamtx.log，也不自写app.pid；单实例互斥依据data_dir公共锁。

/run/isarmg/xcos仅存systemd/MediaMTX临时YAML，由systemd创建0700目录。主服务Requires/After companion，
companion PartOf主服务；停止时主先停、媒体后停。主RuntimeDirectoryPreserve=yes防止提前删除，
媒体Preserve=restart在完整停止时清理；两者TimeoutStopSec=120s给业务排空留余量。
离线命令不依赖/run，持久状态全部位于data_dir，独立录像盘通过recordings_directory配置。

密钥缺失或格式/权限错误会阻止运行，绝不自动修复或重生成。备份数据库时必须同时保存secrets.json。
安装/升级统一使用新包根install.sh；不导入旧env、旧release布局或外部密钥。
check可离线检查，真实systemd重启、TLS信任、NAT/ICE和实际摄像头链路仍须在目标环境验收。
