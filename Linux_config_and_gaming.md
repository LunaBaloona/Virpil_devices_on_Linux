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
* Tested on CachyOS (Yes, an Arch distro...BTW) using since Kernels 6.12 to 6.17RC and SystemD v258 and under. 


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

**Create a system group for joysticks and add your user**
This is a new permissions thing required since SystemD v258. 

```
sudo groupadd --system joystick-users
sudo usermod -a -G joystick-users $USER
```

To add any other users on your PC replace [username] with their username

```
sudo usermod -a -G joystick-users [username]
```

 **Create a udev rules file**
 This will fix detection of our devices in games and fix deadzone for the joystick. 

* Create the file:
 ```
sudo nano /etc/udev/rules.d/99-joystick.rules 
 ```
* Then paste these below: and replace the [Username] your linux username, and [PID] your Joystick's PID - you do not need your throttle PID for this file.
  
```
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="3344", MODE="0660" GROUP="joystick-users"
SUBSYSTEM=="usb", ATTRS{idVendor}=="3344", MODE="0660" GROUP="joystick-users"
```
* Save and exit nano.
  
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
