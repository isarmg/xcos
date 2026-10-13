# 安装与首次运行

仅接受当前 Linux AMD64 GNU/systemd 签名包和固定 ROOT 合同。安装和维护由独立 xssi 交付，server 只有运行入口，不初始化、不执行安装或升级，也不识别代理软件。

默认 ROOT 为 `/opt/isarmg/xcos`。配置位于 `ROOT/config/config.json`，状态默认 `ROOT/state`，应用日志 `ROOT/logs`，临时运行文件 `ROOT/run`。程序和签名资源由 root 管理，服务使用普通 `isarmg-xcos` UID。录像与实例业务数据可以明确外置。配置目录 0700、文件 0600，密钥与数据库保持同组备份。

复制包内 `share/config/config.json.example` 到受保护的输入配置，填写真实 RTSPS 域名和媒体设置。安装公钥及其 DER SHA-256 指纹来自独立可信来源，不能因为包自带公钥就信任它。

```sh
systemd-ask-password 'Initial administrator password:' | sudo ./install.sh \
  --package /absolute/verified-package --root /opt/isarmg/xcos \
  --config /absolute/private-config.json --username admin \
  --public-key /absolute/trusted-public.pem --key-sha256 TRUSTED_DER_SHA256
```

脚本检查发行包、目录、权限和输入，使用签名辅助程序显式初始化新状态，然后生成 systemd 文件并 reload。失败保留受保护事务用于检查或重试；已有状态不被重新初始化。安装成功仍保持停止。

配置 RTSPS 证书、私钥和 `ROOT/config/mediamtx.env` 的 TLS/ICE 参数后启动：

```sh
sudo systemctl enable --now xcos.service
curl --fail http://127.0.0.1:18106/readyz
```

Xcos 和 MediaMTX 是同一写入组；MediaMTX 程序、固定合同和模板也在 ROOT，录像目录可外置。端口和媒体配置见[配置说明](configuration.md)。升级和维护见[运维](operations.md)。
