# slimpend

Background service that enables Slim Pen 2 haptic feedback by binding to bluetooth, hidapi, and evdev.

Requires [IPTSD](https://github.com/linux-surface/iptsd) to be running and the pen to be previously paired.

Automatically listens for connections up/down on bluetooth and starts haptic feedback when bluetooth is detected.
