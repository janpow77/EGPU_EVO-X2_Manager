#!/usr/bin/env bash
# GTT auf 108 GB setzen (Strix Halo AI Max+ 395)
# Kernel 6.17: ttm.pages_limit (NICHT amdttm.pages_limit)
ssh -t janpow@192.168.178.72 \
  "sudo sed -i 's/^GRUB_CMDLINE_LINUX_DEFAULT=\".*\"/GRUB_CMDLINE_LINUX_DEFAULT=\"ttm.pages_limit=27648000 ttm.page_pool_size=27648000\"/' /etc/default/grub && sudo update-grub && sudo reboot"
