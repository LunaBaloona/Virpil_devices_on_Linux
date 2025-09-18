# Getting the Virpil devices to work in Linux and get gaming. 

At the time of writing, the Linux kernel doesn't yet have official drivers from Virpil. As such, the devices are just detected as generic joysticks or more often in WINE, as a joypad/controller. This often results in random missing buttons in games, or missing axis on your joysticks. To solve this, we need to make a few rules in Linux and make a few changes to WINE. 

### Prerequisites for this guide:
* Using a Virpil Joystick, Throttle, and/or HOTAS system connected via USB.
* You have already initialised and calibrated your Virpil devices, and updated their firmware. [A guide for this is available](initialise_and_firmware_update.md)
* You have a working Linux system with a writeable core. (This may not work exactly the same on immutable distros such as Bazzite).
* You know how to edit and save a text file in nano (Ctrl+O to save, then CTRL+X to exit)
* You know how to use Windows or WINE regedit. You will need to search (CTRL+F) and delete sections in there.
* You have already installed the games you want to fix. 

### Assumptions, games, devices in this guide: 
* Hardware tested using "WarBRD-D H.O.T.A.S Bundle - ALPHA Prime Edition". This includes the VPC WarBRD-D, Alpha Prime Grip, MongoosT-50CM3 Throttle (AKA the CM3)
* Games tested are Star Citizen, installed via the LUG with plain WINE. Elite Dangerous installed via Steam.
* Tested on CachyOS (Yes, an Arch distro...BTW) using since Kernels 6.12 to 6.15RC


## Initial Linux setup

 **Find your Virpil device's Vendor ID (VID) and Product ID (PID)**. These are usually displayed in two sets of 4 digits split by a colon (VID:PID)
We already know Virpil's VID is 3344 so we run the following to find the device's PID

```
lsusb | grep '3344:'
``` 

In my example I get
```
Bus 001 Device 003: ID 3344:43f5 Leaguer Microelectronics (LME) R-VPC Stick WarBRD-D
Bus 001 Device 004: ID 3344:8194 Leaguer Microelectronics (LME) L-VPC MongoosT-50CM3
```
So the PID is 43f5 for the joystick, and 8194 for the throttle. Remember whatever you found, as they will be used throughout this guide. 


 **Create a udev rules file**
 This will fix detection of our devices in games and fix deadzone for the joystick. 

* Create the file:
 ```
sudo nano /etc/udev/rules.d/99-joystick.rules 
 ```
* Then paste these below: and replace the [Username] your linux username, and [PID] your Joystick's PID - you do not need your throttle PID for this file.
  
```
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3344", MODE="0660" GROUP="[username]"
SUBSYSTEM=="usb", ATTRS{idVendor}=="3344", MODE="0660" GROUP="[username]"
ACTION=="add", SUBSYSTEM=="input", ENV{ID_VENDOR_ID}=="3344", ENV{ID_MODEL_ID}=="43f5", RUN+="/usr/local/bin/joystick-init.sh"
```
* Save and exit nano. 


**Create the executibe**
You may have noticed we referred to an executable .sh file above. Here's how to create this: 

* First, find the joystick event symlink (only the joystick though, throttle is not needed) by running:

```
ls /dev/input/by-id/ | grep 'VIRPIL'
```

In my example, I get: 

```
lrwxrwxrwx - root 22 May 13:29 usb-VIRPIL_Controls_20250313_L-VPC_MongoosT-50CM3_090C1B00-event-joystick -> ../event20
lrwxrwxrwx - root 22 May 13:29 usb-VIRPIL_Controls_20250313_L-VPC_MongoosT-50CM3_090C1B00-joystick -> ../js1
lrwxrwxrwx - root 22 May 13:29 usb-VIRPIL_Controls_20250313_R-VPC_Stick_WarBRD-D_080E2100-event-joystick -> ../event14
lrwxrwxrwx - root 22 May 13:29 usb-VIRPIL_Controls_20250313_R-VPC_Stick_WarBRD-D_080E2100-joystick -> ../js0
```

We only need the Events part, and of that only the portion for your joystick that starts "usb_" and ends "event-joystick". In my example, I only want the deadzone fix to apply to my joystick, I need to copy `usb-VIRPIL_Controls_20250313_R-VPC_Stick_WarBRD-D_080E2100-event-joystick`. 

*  Now create the executable:

```
sudo nano /usr/local/bin/joystick-init.sh
```

* Paste the following section, remembering to replace the line after "JOYSTICK_SYMLINK=" to match what you found above. Here is my example: 

```
#!/bin/bash

# Set this to your joystick's symlink name (no path)
JOYSTICK_SYMLINK="usb-VIRPIL_Controls_20250313_R-VPC_Stick_WarBRD-D_080E2100-event-joystick"

DEVICE="/dev/input/by-id/$JOYSTICK_SYMLINK"
LOGFILE="/tmp/joystick.log"

{
  echo
  echo "=== JOYSTICK INIT (by symlink) ==="
  echo "Timestamp: $(date)"
  echo "DEVICE: $DEVICE"
  echo "Whoami: $(whoami)"

  if [ -e "$DEVICE" ]; then
    echo "Device found. Applying settings..."
    /usr/bin/evdev-joystick --e "$DEVICE" --d 0 --f 0
    echo "evdev-joystick exit code: $?"
  else
    echo "Device not found at $DEVICE"
  fi
  echo "==============================="
} 2>&1 | logger --skip-empty
```

* Make the file executable by running:

  ```
  sudo chmod +x /usr/local/bin/joystick-init.sh
  ```

* That's the main Linux part done. You may want to reboot at this point. 

## Game fixes - Elite Dangerous

**Find the Steam game ID** 

Assuming we're using steam here, we need to find the Steam game's steam ID, which leads us to the WINE prefix. 

* Right click the game in steam > Properties > Updates. The "App ID" is the game ID. At time of writing, For Elite Dangerous it was 359320. 

**WINE has old configs in the prefix's registry, we need to remove them all**

* Run Regedit from the game's Wine Prefix: (replace [username] and [App ID] for your system and game)
```
WINEPREFIX="/home/[username]/.local/share/Steam/steamapps/compatdata/[App ID]/pfx/" wine regedit
```

*  You may want to backup the reg before you proceed - just in case. (Click Registry in the top left > export)
  
*  In regedit, search for any reference to Virpil's USB Vendor ID: 3344, and/or PID, and delete all references to them. Click `HKEY_LOCAL_MACHINE` and search "VID_3344". Click any matches then delete. Repeat for PID "PID_[yourPIDhere]" (Last bit likely not necessary, but we're in orbit and have a nuke button).  Note that it may take a minute or more for any results. Don't worry if it appears nothing's happening. 
  
*  Close regedit.

**Getting the game to run with fixes**
   
* Return to steam and set properties so HIDRAW uses the VID/PID, for all your devices. These are always in 0xVID/0xPID format, each device separated by comma (replace what you need from my example below)
   -  In Steam, right click game > properties > Launch Options text box: 
```
PROTON_ENABLE_HIDRAW=0x3344/0x43f5,0x3344/0x8194,0x03eb/0x2054,0x03eb/0x2046 SDL_JOYSTICK_HIDAPI=0  %command%
```

That's it. You can now run the game and should have fully working Joystick and Throttle with all buttons and and all axes detected normally. 

## Game fixes - Star Citizen
