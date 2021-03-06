#! /bin/sh

exit # Stubbed

lsmod | grep -q sdio_wifi_pwr && exit 1

insmod /drivers/"${PLATFORM}"/wifi/sdio_wifi_pwr.ko
insmod /drivers/"${PLATFORM}"/wifi/"${WIFI_MODULE}".ko

REM_TRIES=20
while [ "$REM_TRIES" -gt 0 ] ; do
	[ -e /sys/class/net/"${INTERFACE}" ] && break
	REM_TRIES=$((REM_TRIES-1))
	sleep 0.2
done

ifconfig "$INTERFACE" up
[ "$WIFI_MODULE" != 8189fs ] && [ "$WIFI_MODULE" != 8192es ] && wlarm_le -i "$INTERFACE" up

pidof wpa_supplicant > /dev/null || wpa_supplicant -D wext -s -i "$INTERFACE" -c /etc/wpa_supplicant/wpa_supplicant.conf -C /var/run/wpa_supplicant -B

udhcpc -S -i "$INTERFACE" -s /etc/udhcpc.d/default.script -t15 -T10 -A3 -b -q > /dev/null &
