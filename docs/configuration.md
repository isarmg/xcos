# 配置与网络

应用配置为 `/etc/isarmg/xcos/config.json`，归 `isarmg-xcos:isarmg-xcos`，文件0600、父目录0700。
默认配置不存在时可使用默认值与 `XCOS_*` 覆盖；显式文件缺失、权限或格式错误均失败。
优先级：CLI > 产品前缀环境 > JSON > 默认值。旧字段与旧环境别名不支持。

- `bind`：单个SocketAddr，默认 `127.0.0.1:18106`
- `media_callback_bind`：默认 `127.0.0.1:18107`，仅loopback，独立于HTTP入口
- `data_dir`：默认 `/var/lib/isarmg/xcos`，唯一内部持久根；数据库、secrets.json、锁和logs均在其中
- `recordings_directory`：可选独立业务存储盘；默认data_dir/recordings；须由服务UID拥有且0700
- `development`：默认false；开发模式仅允许loopback
- `log_level`：默认info；可选off/error/warn/info/debug/trace，环境为XCOS_LOG_LEVEL
- `trusted_proxies`：默认空数组，只信任直接连接；配置可信代理IP/CIDR后公共层才解析X-Forwarded-For
- `mediamtx_config`、`mediamtx_contract`、`mediamtx_binary`：媒体模板、版本合同和可执行文件的绝对路径
- `public_rtsp_publish_base_url`：客户端可达RTSPS origin，生产禁止明文及loopback
- `public_hls_base_url`、`public_webrtc_base_url`：默认同源 `/media-hls`、`/media-webrtc`

不接收database_url、runtime_directory、jwt_secret或credentials_key。`init` 自动生成独立随机密钥，
写入data_dir/secrets.json；部署配置没有密钥值。运行/check只读取该文件，缺失或损坏即失败，绝不补造。
请把secrets.json与数据库一起备份。重启、升级、管理员密码重置都不更换实例密钥。

应用环境变量只使用XCOS_；可信代理数组用JSON编码的XCOS_TRUSTED_PROXIES。
MediaMTX的MTX_*是外部组件专属配置，不是应用配置别名。

## HTTPS与媒体网络

应用、MediaMTX和持久数据处于同一主机。HTTPS由已有网关终止，并保留Host/Origin：

- Web/API/SSE回源bind（默认18106）
- `/media-webrtc/*`去掉前缀后转发至8889，保留WHEP方法和响应头
- `/media-hls/*`去掉前缀后转发至8888，确保播放列表和重定向保持同源

默认不信任转发IP头。仅填写实际网关地址/CIDR；不要因网关存在就信任任意来源的XFF。
回调18107仅供本机媒体授权，不能作为公网入口。MediaMTX API9997/metrics9998/playback9996保持loopback。

客户端RTSPS8322需要可信证书；WebRTC UDP8189需要浏览器可达的ICE候选和防火墙/NAT规则。
HTTPS代理不能替代RTSPS TLS或UDP连通性。mediamtx.env保留MTX_RTSPENCRYPTION=strict，
配置MTX_RTSPSERVERCERT、MTX_RTSPSERVERKEY、MTX_RTSPSADDRESS和MTX_WEBRTCADDITIONALHOSTS。
不要通过MTX环境覆盖生成的authHTTPAddress或recordPath；这两项由应用JSON唯一产生。
