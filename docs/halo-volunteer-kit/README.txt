PROJECT HALO -- VOLUNTEER WORKER KIT (macOS, Apple Silicon)

What this is: your Mac holds a slice of a large AI model for a few minutes while a machine in
Jack's lab runs the model across the network. Nothing is installed; nothing stays on your machine.
The lab sees only what this window prints. Test prompts are deliberately public and boring.

YOU NEED
  1. An Apple Silicon Mac (M1 Pro or better, 16 GB+). Close big apps first.
  2. Wired Ethernet if you possibly can. Wi-Fi works but adds jitter.
  3. Tailscale (free): App Store -> "Tailscale" -> sign in with the invite link Jack sent you.
     Your Mac joins Jack's private network. Nothing is opened on your router.

TO RUN A TEST
  1. Unzip this folder anywhere.
  2. Open Terminal, drag start-halo-worker.sh into it, press Return.
     (First time, macOS may ask you to allow the program -- System Settings -> Privacy & Security
      -> "Open Anyway".)
  3. It prints your Tailscale address (100.x.y.z) -- send that to Jack once.
  4. Leave the window open. Jack runs the test from the lab; you'll see a few lines scroll by.
  5. When Jack says done: Ctrl-C. Between tests, Ctrl-C and run it again (it only takes one
     connection per run).

WHAT YOU'LL GET BACK
  Your machine's share size, the model, prompt and generation speed, and how that compares to the
  lab's own numbers -- i.e. what a member at your distance contributes to a pool.

Questions: jack@happyjack.media
