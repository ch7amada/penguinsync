I want to build an android linux sync app.
# Stack to use
* nativa android app using kotlin and compose 
* on linux a tui tool and a linux daemon to run in the background using rust (not sure) 
# Phase 1: Establish base 
* local network connection between android and linux. maybe linux show qr code and android connects using it 
* automatic reconnect 
* connection is encrypted
* investigation about clipboard sync in the background on android 
* investigation about gnome clipboard sync in the background

# Phase 2: sync clipboard gnome (wayland) and android 
* read clipboard on android and gnome 
* send clipboard autonmatically to the other device (android to linux and vice versa)

# Phase 3: Encryption
* encrypt send data between the devices 

# Phase 4: send files 
* send files between the two devices
* investigation how to effeciently send big files between the two 
* sending files should be automatically 
 * maybe build-in nautilus file explorer, where you right click and select send with penguinsync 

# Notes:
* Focus first only to get a product on gnome wayland 
* other distros come later 
* clean architechture and modular system for expanding the app in the next stages
