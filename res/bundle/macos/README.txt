Solarxy for macOS
=================

Quick start
-----------
1. Drag Solarxy.app to the Applications folder in this window.
2. Open Applications > Solarxy. The first launch is blocked by macOS
   Gatekeeper because Solarxy is not yet code-signed with an Apple
   Developer certificate. See "First launch" below.
3. (Optional) Double-click "Install CLI.command" to add the `solarxy`
   command to your PATH. You will be prompted for your sudo password.

First launch — bypassing Gatekeeper
-----------------------------------
macOS refuses to open unsigned apps by default. To allow Solarxy:

   1. Double-click Solarxy.app. macOS shows "cannot be verified". Click Done.
   2. Open System Settings -> Privacy & Security.
   3. Scroll to Security. Click "Open Anyway" next to the Solarxy message.
   4. Confirm with your password when prompted.

macOS remembers this choice. Subsequent launches do not prompt.

Code signing
------------
Solarxy 0.5.0 is shipped unsigned. An Apple Developer certificate is on the
roadmap for 0.7.0, at which point Gatekeeper will accept the app without the
bypass above.

License
-------
MIT. See the project repository at
https://github.com/marko-koljancic/solarxy for source and license text.
