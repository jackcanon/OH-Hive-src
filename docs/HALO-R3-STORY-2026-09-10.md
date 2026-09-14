# Four machines, one model: the day the 70B ran

*Project Halo, Loki's Lab — September 10, 2026*

At 11:26 this morning a 42.5-gigabyte language model finished a benchmark on hardware that, by any single machine's spec sheet, cannot run it. Llama 3.3 70B, four-bit, spread across four computers on the lab's wired network: an M4 Max hosting, an M4 Pro, an RTX 4070 in a Linux box, and a four-year-old M1 Pro laptop holding the last six gigabytes. Three clean repetitions. Exit code zero. Five tokens per second.

That last number is the one to be honest about, and we'll get to it. But first the part worth being excited about, because it's the question Project Halo was created to ask: can a group of ordinary machines pool their memory to load an agent "blob" that none of them could hold alone? As of today, on a LAN, the answer is yes.

## What actually happened

The placement was Overgaard 15 GB, Odin 13, Heimdall 9, Jotunheim 6 — forty-three gigabytes of budget for a forty-two-and-a-half gigabyte model, every share sitting inside the memory each machine had measured it could actually give, not the number on the box. The run was launched from HaloBench on Midgaard, with Overgaard doing the hosting over SSH and the app's timeline watching each stage go green: workers restarted, host GPU up, then a long quiet stretch while Overgaard read the model off disk and pushed 28 gigabytes out to the three workers, then the results table.

Prompt processing came in at 51.2 tokens per second. Generation at 4.97. The night before, before any of this had run, the prediction written down for exactly this test was "2–5 tok/s." It landed at the top of that range.

It is also the first time a Metal host, a CUDA worker, and two more Metal workers have computed a single model together in this lab. The 4070 in the middle of an Apple Silicon pipeline is not a detail; it's the shape Halo's real fleet will have.

## Why it took two days

The same test had failed four hours earlier. So had every other attempt at a model bigger than fifteen gigabytes since the experiment began — the 32B at two quantizations, the 70B, on two different hosts, in two-way and three-way splits, always with the same terse host-side error and always with the workers sitting there healthy and silent.

The cause turned out to be one behavior and one hidden log. With llama.cpp's default memory-mapped loading on Apple Silicon, the host maps the *entire* model into GPU memory no matter how the layers are split, and the workers get their share on top of that. Pooling never reduced what the host had to hold, so any model bigger than the host's working set failed regardless of how many machines were helping. And llama-bench silences the library's own error messages unless you ask for them, which is why it looked like a mystery for a day and a half instead of an afternoon.

Turn off mmap — one flag, `-lm none` — and the host holds only its share. Forty minutes after that was understood, the 70B passed.

## The caution

Five tokens per second is not a chat experience. It is roughly a word every quarter-second, slower than a person reads. Halo has always framed this tier as "models that otherwise can't run at all — batch agent work, not conversation," and today's number says that framing was right, not that it was pessimistic. The speed follows the physics the research predicted: every added hop costs decode time, and the curve from today's runs is unambiguous — a 14B split two ways ran at 18.7, three ways at 14.6, the 32B at 11.9, the 70B four ways at 5.0.

All of this happened on a wired gigabit LAN with sub-millisecond latency between machines. That is the easy case. Halo's actual ambition — members' machines pooling across the open internet — adds fifty to a hundred milliseconds per hop, and nothing today speaks to that yet.

It's also three passes. The run was repeated half an hour later with freshly restarted workers and came back at 51.27 and 4.99 — within half a percent of the first. Then the placement was changed, and that third run is the one that turns a number into an understanding.

## The laptop in the corner

Jotunheim is a 2021 MacBook Pro with an M1 Pro and sixteen gigabytes of memory. In the lab's solo profiles the night before, it ran every model at almost exactly half the speed of the M4 Pro next to it — the memory bus is the reason, 200 GB/s against 273 — and it could give a pool about ten usable gigabytes. It is the weakest machine in the building that was still worth plugging in, and it is exactly what most of Halo's member fleet will look like.

In the first two 70B runs it held six gigabytes, the last slice that made the model fit. For the third run it was dropped and those six gigabytes moved onto the host. Same model, same everything else, one fewer machine: generation went from 4.99 to 6.67 tokens per second, and prompt processing from 51 to 68. A third faster.

Per token, that's 200 milliseconds down to 150. So Jotunheim's presence had been costing the pool about fifty milliseconds on every token — its share of the layers running on a slower bus, plus one more trip across the network. The night before, from the solo numbers alone, the estimate written down for that cost was "about 40 milliseconds." It came in at fifty. A prediction made from one machine's profile held to within ten milliseconds on a four-machine pipeline, which is the kind of thing that makes a scheduler possible rather than a guess.

What Jotunheim teaches is not "weak machines are bad." Without it, on the night the M4 Max was busy, the 70B would not have fit anywhere at all — it was the difference between running and not running. The lesson is about *when* a weak node earns its place: only when the pool cannot hold the model without it. The moment the host could absorb its share, keeping it in the pipeline cost a quarter of the throughput for nothing. A Halo scheduler should build the smallest pool that fits the model, fastest machines first, and reach for the laptop in the corner only when there is no other way to load the thing. That rule now has a measurement behind it.

## The rest of the caution

Three placements on one LAN is still not a characterization, and an hour before the first pass a different no-mmap run lost its connection to a worker mid-upload for reasons that haven't been explained. Nothing here has been tried against the things that will actually go wrong in a volunteer fleet: a machine dropping out while it holds a shard, a laptop closing its lid, a worker that's slower than the others.

## What's next

The next tests are the ones designed to break it. R7 kills a worker in the middle of a long generation and records exactly what the host does — the expectation is that the whole run dies with no recovery, and that expected failure is the finding, because it defines the checkpoint layer Halo's scheduler would have to build. R8 runs the same prompt at temperature zero through the all-Metal path, the mixed Metal-and-CUDA path, and a CPU baseline, and diffs the outputs, to see whether heterogeneous hardware produces the same tokens. Both use the placement that worked today.

Then Test 02, the one that matters most for Halo's real shape: Heimdall can artificially delay its own network link, so the same 70B split can be re-run at 25, 50, and 100 milliseconds of added latency on one hop. That produces the lab's own version of the latency curve the distributed-inference literature describes, on the lab's own hardware, and it will say plainly what a member in another city contributes to a pool versus what they cost it.

Everything learned today that changes how Hive v2 gets built — no-mmap on the host, the smallest-pool-first placement rule, measured rather than nominal capacity, how workers are launched, what the runner has to log — is written up separately as integration requirements (`docs/HALO-V2-INTEGRATION-LESSONS.md`) and tracked as work in Cmd Work, so none of it lives only in the story.

Today the lab pooled four machines and loaded something none of them could carry. Tomorrow's job is to find out how it fails.
