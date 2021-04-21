Control lights or anything else in your BYOND game via DMX with ArtNet! Cool!

# Setting up the Rust side
1. Clone this repo somewhere
2. Run `cargo build --release`
3. Copy `stagehand.dll` from `build/target/release` to the directory wih your dmb

# Setting up the DM side
1. Ensure your codebase is compatible with [auxtools](https://github.com/willox/auxtools) and [auxcallback](https://github.com/MCHSL/auxcallback), instructions below
2. Define the following procs:
    ```
    /proc/enable_stagehand()
	    CRASH("enable_stagehand not hooked")

    /proc/dmx_register(source, procpath, universe, start_channel, footprint)
	    CRASH("dmx_register not hooked")
	```
	They will be hooked with auxtools.
3. Hook up your lights to receive updates. Here is an example of a simple fixture:
    ```
    /datum/dmx_receiver
    	var/r = 0
    	var/g = 0
    	var/b = 0

    /datum/dmx_receiver/New()
    	. = ..()
    	dmx_register(src, .proc/set_color, 1, 1, 3)

    /datum/dmx_receiver/proc/set_color(nr, ng, nb)
    	world << "([nr], [ng], [nb])"
    	r = nr
    	g = ng
    	b = nb
	```
	Pretend it's an RGB light. Calling `dmx_register(src, .proc/set_color, 1, 1, 3)` registers the fixture on universe no. 1. The fixture will listen to changes on channels 1, 2 and 3 (start_channel 1 with footprint of 3). When a controller updates a channel, e.g. changes channel 1 from 50 to 100, `set_color()` will be called with the values of channels 1-3.
	Instead of datums, you'll most likely want to register atoms to be controlled.
4. Call `auxtools_init` in the library DLL, e.g. `call("stagehand.dll", "auxtools_init")()`. If you're missing anything, the function will return a description of the error, otherwise, it will return "SUCCESS".
5. Call `enable_stagehand()` to begin receiving data from your controller. By default, this library listens on all interfaces, port 6454.

# Auxtools and Auxcallback
This library uses the above to libraries to interface with DM. You will need to define the following procs in your codebase:
```
//Used to display errors in hooks, you can modify the contents if you want
/proc/auxtools_stack_trace(msg)
    CRASH(msg)

//Call to process callbacks scheduled from rust threads
/proc/process_callbacks()
    CRASH("process_callbacks not hooked")
```

`process_callbacks()` needs to be called often, ideally once a tick. If using a master controller, you can create a subsystem for it. It accepts 2 arguments to control which callbacks to run and for how long; See [here](https://github.com/MCHSL/auxcallback/blob/3aee2d354cc15e1c879895bea0a88eba8ca58803/src/lib.rs#L132) for an explanation.

In addition, remember to call `auxtools_shutdown`, for example `call("stagehand.dll", "auxtools_shutdown")()` upon stopping or restarting the server. Not doing so will most likely immediately crash it.
