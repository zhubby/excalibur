# Adds device_agent and bridge to systemd

# To be executed after the rootfs is extracted to /mnt/next_root
cp /mnt/download/systemd/device_agent.service /mnt/next_root/etc/systemd/system/
if [ -f /mnt/next_root/etc/systemd/system/multi-user.target.wants/device_agent.service ]
then
	unlink  /mnt/next_root/etc/systemd/system/multi-user.target.wants/device_agent.service 
fi
ln -s /mnt/next_root/etc/systemd/system/device_agent.service /mnt/next_root/etc/systemd/system/multi-user.target.wants/device_agent.service

cp /mnt/download/systemd/bridge.service /mnt/next_root/etc/systemd/system/
if [ -f /mnt/next_root/etc/systemd/system/multi-user.target.wants/bridge.service ]
then
	unlink  /mnt/next_root/etc/systemd/system/multi-user.target.wants/bridge.service 
fi
ln -s /mnt/next_root/etc/systemd/system/bridge.service /mnt/next_root/etc/systemd/system/multi-user.target.wants/bridge.service

cp /mnt/download/systemd/startup.service /mnt/next_root/etc/systemd/system/
if [ -f /mnt/next_root/etc/systemd/system/multi-user.target.wants/startup.service ]
then
	unlink  /mnt/next_root/etc/systemd/system/multi-user.target.wants/startup.service 
fi
ln -s /mnt/next_root/etc/systemd/system/startup.service /mnt/next_root/etc/systemd/system/multi-user.target.wants/startup.service
