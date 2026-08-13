# slimpend

Background service that enables Slim Pen 2 haptic feedback by binding to bluetooth, hidapi, and evdev.

Requires [IPTSD](https://github.com/linux-surface/iptsd) to be running and the pen to be previously paired.

Automatically listens for connections up/down on bluetooth and starts haptic feedback when bluetooth is detected.

## installation

Prerequisites - 

* IPTSD running
* User is in `input` group (or at least has access to IPTSD input device)
* **important** - Slim Pen 2 has been paired before

1. `cargo install --path .`
2. `cp slimpend.service ~/.config/systemd/user`
3. `systemctl --user daemon-reload`
4. `systemctl --user enable --now slimpend`
