# live long and Prospero

This is a response to [Matt Keeter's Prospero Challenge][prospero-challenge].
Matt has defined a small language for describing shapes using [Constructive
Solid Geometry][]; a friend of mine described this as "like SVG if
mathematicians invented it." If you have a shape that's been described this
way, you can decide whether a point is part of that shape by evaluating a
mathematical expression on the coordinates of that point. So to draw a complete
image of the shape, you need to evaluate the expression on the coordinate
of each pixel in the image. If the expression is complicated or you want a
high-resolution image, this could get very slow.

[prospero-challenge]: https://www.mattkeeter.com/projects/prospero/
[Constructive Solid Geometry]: https://en.wikipedia.org/wiki/Constructive_solid_geometry

The Prospero Challenge is to draw a shape that Matt has provided, which happens
to be the text of a snippet from Shakespeare's "The Tempest", and to do it as
quickly as possible. Or at least in some way that's interesting. Or just have
fun playing around with it; that's awesome too.

I want to start by saying that I love the setup Matt arranged for this
challenge. It's well suited to be anything from an exercise done in an
afternoon, to weeks of fiddling. He begins by demonstrating that it's possible
to solve the challenge with a very short Python program, making it easy to get
started with your own experiments if you know a little Python. It's no wonder
that quite a few people's solutions are already in the gallery given that Matt
has made the challenge so welcoming.

## Experiment goals

The main question I wanted to investigate as I took on this exercise was whether
*memoization* would make a significant difference to drawing speed. Many other
solutions have used parallel processing, either on multi-core CPUs or on GPUs,
to gain linear speedups on this problem, but I don't think anyone else has yet
investigated time-space tradeoffs, and I thought that would be an interesting
thing to experiment with.

I also wanted to just use this as an exercise in writing a small compiler
from scratch. I previously was a maintainer for [Cranelift][], a fast compiler
backend used in projects like [Wasmtime][] as well as an experimental Rust
codegen backend, and I was inspired by seeing that someone had used Cranelift
to put together a remarkably concise solution for this challenge. But Cranelift
is designed to handle the complexity of general-purpose programs, while this
challenge involves programs which are astonishingly simple; they're really only
challenging because they get run a *lot*. So I took the opportunity to try out
some ideas that are tricky in a full compiler like Cranelift.

[Cranelift]: https://cranelift.dev/
[Wasmtime]: https://wasmtime.dev/

## Intermediate representation transformation passes

The `src/ir/` directory contains several transformation passes which output
the same intermediate representation as Matt's input, so they can be chained
together as desired for different experiments. There are corresponding example
programs in `examples/` for running one pass at a time, and you can pipe them
together if you want.

Two other example programs may be useful while experimenting with these passes:

- `cargo run --example print` does not modify the input program at all but
  just prints it out as parsed. My parser discards all the variable names
  from the input in favor of just sequentially numbering each instruction's
  results, so the `print` example is useful if you want to diff the output of a
  transformation pass against the original input to see what it changed.

- `cargo run --example interp` is an interpreter for Matt's language. It's quite
  slow, but useful for checking whether transformations broke the input program.

### Memoization

Matt's Python sample program has an interesting property not shared by most of
the "serious" solutions: for any subexpression that depends only on the X input
(or similarly, only on the Y input), it computes a 1-dimensional vector of the
values of that subexpression. When such expressions are combined into larger
expressions that depend on both X and Y, the result is a 2-dimensional matrix,
but until then it uses less space and avoids recomputing the same subexpression
multiple times.

I wanted to do the same thing as an explicit code generation strategy during
compilation. So my compiler splits the input program into four parts:

- a pool of constants, which can be stored in read-only data,
- a function for computing all subexpressions which depend only on X, and
  storing those partial results in a buffer,
- a similar function for subexpressions which depend only on Y,
- and a function which combines results from the other parts into the final
  result.

I then wrote a test harness (in `examples/x86-harness.c`) that calls the
generated `x` function on each X-value once up front; then, for each row of
output, it calls the `y` function on that row's Y-value; and finally it calls
`xy` on successive parts of the X buffer together with the current Y buffer.

`cargo run --example memoize` reads an input program in Matt's format and prints
the split version, including new instructions for loading and storing in the
intermediate buffers.

### Reassociation

With memoization implemented, I found that there were cases where
single-variable expressions were combined into multi-variable expressions sooner
than necessary. For example you could have an expression like this:

```
(x * 2 + y * 3) + (x ^ 2 + y ^ 2)
```

After memoization, this expression requires storing two partial results from `x`
and two more from `y`, to be combined later in `xy`.

Because addition is associative, that expression can be rearranged like so:

```
(x * 2 + x ^ 2) + (y * 3 + y ^ 2)
```

Now the entire first half can be computed all at once in `x`, storing only one
partial result, and similarly for `y`. This reduces the total amount of memory
required for buffers between the stages, and reduces memory bandwidth and cache
occupancy for reading and writing those buffers. It also moves more operations
to the `x` and `y` functions, which run `size` times per image, instead of the
`xy` function which runs `size^2` times.

I tried several different ways to implement reassociation before settling on
the current implementation, which is simpler and works better than my previous
attempts.

`cargo run --example reassociate` reads an input program in Matt's format,
applies this transformation, and prints it out again in the same format.

### Simplification

This pass applies a few algebraic rules, normalizing expressions so that
negation is always at the outermost possible subexpression. For example, `(-x) +
(-y)` is rewritten to `-(x + y)`, and `x * -y` is rewritten to `-(x * y)`. There
are rules for min and max as well, which are slightly trickier: `min(-x, -y)`
is the same as `-max(x, y)`, for example. These rules minimize the number of
negation instructions in the program, with no increase in other instructions.

This pass also identifies instructions which compute the same value as an
earlier instruction, and deletes the redundant instructions in favor of reusing
the earlier results. In my implementation I referred to this as "GVN" (Global
Value Numbering), because that's what we called it in Cranelift; but I saw
another person responding to this challenge called it "hash-consing", which is
the same idea discovered in a different historical context.

To make naïve GVN more effective, this pass sorts the operands of commutative
instructions so we only have to check if two instructions are exactly identical.

That doesn't work for subtraction, which isn't commutative. Instead, it checks
whether there's another subtraction with the arguments in the opposite order.
If so, it replaces this subtraction with the negation of the result of the
earlier one. This is an improvement because it means only one value, the result
of the subtraction, needs to be saved for later, instead of having to save both
operands; that reduces register pressure. It also reveals more opportunities to
apply the above algebraic simplifications.

Finally, this pass checks for `square a` instructions where `a` is defined by
a `neg b` instruction. Since `(-x)^2` is equal to `x^2`, we can skip over the
negation and rewrite to `square b` directly. Sometimes the `neg` instruction
has no other uses and can be deleted entirely. This applies fairly often in the
Prospero Challenge.

`cargo run --example simplify` reads an input program in Matt's format,
applies this transformation, and prints it out again in the same format.

### Reordering

I had a suspicion that the order in which instructions are listed would have a
big impact on register pressure in the compiled version, so I tried a simple
topo-sort on them. In my current implementation this actually makes things worse
so I haven't experimented with it much.

For a while, my reassociation algorithm would leave some instructions depending
on the results of later instructions, which breaks assumptions everywhere
else. I was trying to do reassociation in-place rather than copying all the
instructions to a new array, and it is sometimes impossible to do that without
getting dependencies going the wrong way around. So this reordering pass was
also useful for fixing up the output from reassociation. However, that is no
longer necessary.

`cargo run --example reorder` reads an input program in Matt's format, applies
this transformation, and prints it out again in the same format.

## Code generation

The `src/codegen/` directory contains the details of transforming the
intermediate representation into executable code for a specific target platform.
At the moment I've only implemented support for the x86-64 instruction set and
operating systems using the System-V ABI, such as Linux.

### Register allocation

The problem of deciding which CPU register to store each intermediate value in
is not particularly architecture-specific, so I've written an implementation
that is generic and just parameterized over how many registers your target has.

(As a side note, one common misconception about register allocation is that it
is an NP-Complete problem, and that this is why it's difficult to do well. This
belief comes from the fact that one way to do register allocation is to reduce
it to graph coloring, which is an NP-Complete problem. However, for programs in
the SSA form that most compilers use now, the interference graph that you need
to color has a special shape called a "chordal" graph, and chordal graphs can be
colored in polynomial time. Register allocation is indeed difficult to do well,
but more due to engineering complexity than asymptotic complexity.)

Cranelift recently gained an alternative register allocator inspired by an
algorithm called the ["Solid State Register Allocator"][SSRA] (SSRA), and I
remembered just enough about that to think it might be a good choice for this
problem. As it turns out, SSRA was developed by… Matt Keeter, the author of this
Prospero Challenge. I now understand what motivated him to do that!

[SSRA]: https://www.mattkeeter.com/blog/2022-10-04-ssra/

An important thing to understand about SSRA is that it works in a single pass
over all the instructions. When it sees an instruction, it decides immediately
what registers to assign to its operands and results, and then moves on to the
next instruction. (It's also notable that it works in reverse order, which is
key to understanding the algorithm, but not to understanding the rest of my
notes here.)

So I began with SSRA, but I've made two significant changes to the algorithm.

#### Leave values in memory

First, once I've allocated a memory slot to spill a value into, I keep it there.
At various times a copy of it may also live in a register, but the value is only
stored to memory once, right after it's computed. This has some advantages:

- I can initialize the register allocation state so that every value which is
  either loaded from an input buffer or stored into an output buffer has that
  location assigned to it from the beginning. Then all the loads and stores are
  generated during register allocation, in full awareness of register pressure.
  This would not work in a general-purpose compiler like Cranelift, but in this
  simplified situation we know every location is either read-only or just has
  to have the right value written by the time the function returns, and in both
  cases we have a lot of freedom to reorder memory accesses.

- This reduces memory accesses because a value that got spilled to memory and then
  reloaded doesn't need to be written to memory again.

- It seems to have dramatically simplified my implementation. The original
  SSRA has 18(!) special cases when allocating registers for a two-operand
  instruction. My implementation just has to call `get_output_reg` and then call
  `get_reg` twice. But it's possible that my simplifications would work in the
  original setting as well; I haven't felt like thinking too hard about it.

The trade-off is that there may be more values kept in memory at the same time,
and so this choice can result in using more memory, more memory bandwidth, and
more cache lines. But I think the trade-off was worthwhile in this case.

#### Load sinking

The other significant change I made to SSRA was to integrate support for an x86
optimization called "load sinking" into register allocation. Many instructions
on x86 can read one of their source operands from either a register or from
memory. So under the right circumstances, you can replace a pair of instructions
like these:

```
# load from memory at address %rdi+0x20 into register %xmm9
vmovaps 0x20(%rdi),%xmm9
# add the loaded value to %xmm14 and put the result back in %xmm14
vaddps %xmm9,%xmm14,%xmm14
```

with a single instruction like this:

```
# load from memory and use immediately as an operand to add
vaddps 0x20(%rdi),%xmm14,%xmm14
```

Freeing up a register this way is a huge advantage if you need every register
you can get, and for the Prospero Challenge, we do. The downside of doing so is
that if you need the value again later, you'll have to load it again; in that
case, keeping it in a register could be faster.

In Cranelift, load sinking is only performed for values which only have one
use, along with other constraints. Any value used at least twice goes in a
register, which might get spilled back into memory on the stack during register
allocation. That is the safe approach, and is required for loads with side
effects, but this exercise was an opportunity for me to try something different.

Integrating load sinking into register allocation allows deferring the decision
about whether to use a register until we actually know how much register
pressure we have. In this setting, when register pressure is high, it may be
worth loading from the same address multiple times if that means we have another
register free for something else. (If you're familiar with compilers, you might
think of this like rematerialization for loads.)

This integration also allows sinking loads from spill slots, which are only
known during register allocation. So that's neat.

The approach I settled on is that instructions which can have a load sunk into
them always do. But the code generator informs the register allocator of where
the instruction is in the output, and can go back and patch the instruction to
use a register instead if the allocator later decides that's a good idea.

We may discover the load is never used again, in which case we were right not to
spend a register on it. But if we find another use, how can we pick a register
for an instruction we already allocated and moved on from?

My trick is to keep track of which registers were either read or written since
that point. If some registers haven't been used by any of the intervening
instructions, then we can safely pick any of those. But if all of the registers
have been used in the meantime, then again, we were right not to spend a
register on that load, because apparently we needed every register we could get.

Tracking dirty registers efficiently, then, is important. But I realized I could
implement every operation I needed in constant time and, given that the number
of registers on a target does not depend on the number of instructions we're
compiling, also in constant space.

The idea, in brief, is to assign an increasing counter value to each sunk load.
Then, every time a register is used, note that all counter values less than the
next one can no longer use that register. By keeping these "dirty-before" values
in an array indexed by register, this update is just a couple of memory accesses.

Collecting the set of available registers requires checking each "dirty-before"
value, but again, the number of registers is constant and not very big, so this
is fast too.

The register allocator needs to remember some information about each sunk
load, too, namely what offset the code generator said it would need to
patch. But registers get used at a pretty rapid pace, so the odds that a sunk
load will need to be patched drop quickly as more sunk loads are emitted.
Therefore, I keep a fixed-length queue of loads and discard the oldest when
it fills. Empirically, a 32-element queue was more than enough to never miss
an opportunity. Pushing onto the front of a ring-buffer that's never resized
is also a constant-time operation.

My final trick was to lazily free references to loads that have either been
dropped from the queue or no longer have any registers available. In addition to
the patch offset, I also store in the queue the SSA value ID of the instruction
whose result got sunk. (This is either an explicit `load` instruction, or some
other instruction where its result has been spilled to the stack and we need to
reload it.) I also record which of the 32 queue entries was used in the register
allocation state for that value. (This helps keep the allocation state small;
it went from 4 bytes to 6 bytes per value with this change.) So the next time
I need to look up that value, I can go directly to the right entry in the queue
and check if it still refers to the same value.

Actually, I think in this case that it's probably better to always sink loads
whenever possible and skip the extra complexity of patching already-emitted
instructions. At some point I was implementing all of this solely because I'd
figured out how and wanted to try it. Here's what I currently think:

- If two uses of the same address are far apart, then keeping the value in
  a register ties up that register for a long time, which probably hurts
  performance. So load-sinking is good.

- But if the two uses are close together, the value is likely still in
  L1 cache at the second load. According to [Agner Fog's Optimization
  Manuals][agner-manuals], "Reading from the level-1 cache takes approximately
  3 clock cycles." ([Volume 2][agner-vol2], section 11) So keeping the loaded
  value in a register in that case saves about 3 clock cycles, which is not much
  of an advantage, especially if there's plenty of other work for the CPU to do
  while waiting. Therefore load-sinking is, perhaps not actually good in this
  case, but probably not that bad either.

[agner-manuals]: https://www.agner.org/optimize/#manuals
[agner-vol2]: https://www.agner.org/optimize/optimizing_assembly.pdf

### x86-64 codegen

`cargo run --example x86` reads an input program in Matt's format and writes x86
assembly source text to stdout.

I chose to print textual assembly language for the GNU Assembler, rather than
dealing with x86 instruction encoding. This has meant that so far I can't
easily implement a JIT, and have done all my experiments in an ahead-of-time
compilation mode, unlike several other submissions for this challenge. I might
revisit that but this has been good enough for exploring the questions that I
was most interested in.

The modern x86-64 SSE/AVX instructions that everyone uses now for floating-point
math operate in the vector registers. As a result, once I had scalar math
working, vectorizing my compiler's output was almost as easy as changing an "s"
("scalar") to a "p" ("packed") in each instruction. I just had to be careful to
group multiple values together in memory so that vector loads would be able to
fetch a whole vector at a time.

Most of the instructions in Matt's language have single-instruction
implementations available on x86, except that this architecture doesn't have a
floating-point negation instruction. My first solution was to reserve a register
that always holds 0.0, and subtract from that whenever negation was called for.
However, pinning a register to hold an otherwise useless value bothered me.

Once I implemented load-sinking, I wanted to put this constant in the constant
pool and let the register allocator decide when it's worth keeping in a
register. Unfortunately, only the second operand of the subtraction instruction
can reference memory, and I needed the constant to be the first operand.

However, a different way to negate floating point numbers is to XOR a 1 into
their sign bit. (This is what GCC does.) And since XOR is commutative, I was
guaranteed to be able to put the constant into the right operand to allow
load-sinking.

On the Prospero example, this left the new constant in a register for the entire
duration of the `x` and `y` functions, but in `xy`, the register allocator moved
it into different registers at different times, and sometimes even used load
sinking for this constant rather than putting it in a register at all.

## Miscellaneous

Matt's demo used [Netpbm][] format to make it easier to output the images.
Specifically, he chose the PGM ("Portable Gray Map") format. I switched to PBM
("Portable Bit Map") because it's one-eigth the size. This probably made no
practical difference but I learned more about Netpbm. It's a great format for
this kind of thing; I previously used it in [another exercise][].

[Netpbm]: https://netpbm.sourceforge.net/
[another exercise]: https://jamey.thesharps.us/2011/11/18/heuristic-search-flood-it/

## Results

I've measured the effects of various strategies within this implementation, on
runtime, code size, stack frame size, and quantity of loads and stores.

I also verified that all tested variations produce identical output at 2048x2048
resolution.

### Runtime

I've done these tests on an 11th Gen Intel(R) Core(TM) i7-1185G7, which I've
forced to run at 3.00GHz to avoid frequency scaling effects. I [moved other
processes and interrupts off both hyperthreads of one core][cpu-isolation] and
ran [hyperfine][] on that core to compare the different code-generation options
that I've implemented.

[hyperfine]: https://github.com/sharkdp/hyperfine
[cpu-isolation]: https://github.com/bytecodealliance/sightglass/blob/main/docs/cpu-isolation.md

Keep in mind that I'm not using threads in any of these tests. All comparisons
are for single-core performance only.

#### Memoization and load-sinking

In the first experiment, I used `prospero.vm` as my test input, but first
applied the `simplify` and `reassociate` transformations to it. I enabled
vectorized codegen, which in this version uses 128-bit AVX instructions. I
compared `--memoize yes` versus `--memoize no`, and for each mode, also tried
all five `--sink-loads` modes. I tested rendering at 2048x2048 resolution.

```
  prospero-memo-yes-vector-yes-sink-all 2048 ran
    1.00 ± 0.00 times faster than prospero-memo-yes-vector-yes-sink-prefer-dead 2048
    1.00 ± 0.00 times faster than prospero-memo-yes-vector-yes-sink-require-dead 2048
    1.00 ± 0.00 times faster than prospero-memo-yes-vector-yes-sink-spill-any 2048
    1.04 ± 0.00 times faster than prospero-memo-yes-vector-yes-sink-none 2048
    1.23 ± 0.00 times faster than prospero-memo-no-vector-yes-sink-require-dead 2048
    1.23 ± 0.00 times faster than prospero-memo-no-vector-yes-sink-prefer-dead 2048
    1.23 ± 0.00 times faster than prospero-memo-no-vector-yes-sink-spill-any 2048
    1.24 ± 0.00 times faster than prospero-memo-no-vector-yes-sink-all 2048
    1.31 ± 0.00 times faster than prospero-memo-no-vector-yes-sink-none 2048
```

We can see that memoization is about 23% faster than without, and also that
every load-sinking mode I implemented is about equally fast and is slightly
better than not sinking loads.

#### Vectorization

In the second experiment, I verified that vectorization has the expected linear
speedup from processing four times as many points per cycle, regardless of the
rendered image size and regardless of whether memoization is enabled.

Note that the load-sinking optimization does not work unless vectorization
is enabled, because the arithmetic instructions try to load a whole vector
at a time. So to keep this comparison fair I've only tested with load-sinking
disabled.

The effect of vectorization is almost exactly as predicted.

```
  prospero-memo-yes-vector-yes-sink-none 1024 ran
    3.93 ± 0.04 times faster than prospero-memo-yes-vector-no-sink-none 1024

  prospero-memo-yes-vector-yes-sink-none 2048 ran
    3.89 ± 0.02 times faster than prospero-memo-yes-vector-no-sink-none 2048

  prospero-memo-no-vector-yes-sink-none 1024 ran
    4.00 ± 0.06 times faster than prospero-memo-no-vector-no-sink-none 1024

  prospero-memo-no-vector-yes-sink-none 2048 ran
    4.04 ± 0.02 times faster than prospero-memo-no-vector-no-sink-none 2048
```

#### Simplification and reassociation

For the third experiment, I compared some of the above tests against identical
code generation configurations but without applying the `simplify` and
`reassociate` transformation passes first.

These passes provided a 21% improvement when both memoization and load-sinking
are turned on, and nearly as much (18%) if load-sinking is disabled.

```
  prospero-memo-yes-vector-yes-sink-all 2048 ran
    1.21 ± 0.00 times faster than prospero-original-memo-yes-vector-yes-sink-all 2048

  prospero-memo-yes-vector-yes-sink-none 2048 ran
    1.18 ± 0.00 times faster than prospero-original-memo-yes-vector-yes-sink-none 2048
```

However, without memoization, they did not have much effect: 8% in conjunction
with load-sinking, and only 3% without load-sinking.

```
  prospero-memo-no-vector-yes-sink-all 2048 ran
    1.08 ± 0.00 times faster than prospero-original-memo-no-vector-yes-sink-all 2048

  prospero-memo-no-vector-yes-sink-none 2048 ran
    1.03 ± 0.00 times faster than prospero-original-memo-no-vector-yes-sink-none 2048
```

#### Scaling with image size

A straightforward implementation that is saturating the computational resources
it has available to it should take time linear in the number of pixels rendered.
Since the output images are square, doubling the number of pixels along one
axis quadruples the number of pixels total. Therefore we can predict that each
doubling will take roughly four times as long, too. That holds true here for the
non-memoized version.

```
  prospero-memo-no-vector-yes-sink-all 256 ran
    3.93 ± 0.09 times faster than prospero-memo-no-vector-yes-sink-all 512
   15.45 ± 0.28 times faster than prospero-memo-no-vector-yes-sink-all 1024
   61.43 ± 1.00 times faster than prospero-memo-no-vector-yes-sink-all 2048
```

I wasn't sure what to expect for the memoized version, though. Memoized
instructions only run twice as often when the resolution doubles, while the main
loop still runs four times as often. But it turns out that the memoized version
also scales linearly in the number of pixels rendered.

```
  prospero-memo-yes-vector-yes-sink-all 256 ran
    3.88 ± 0.11 times faster than prospero-memo-yes-vector-yes-sink-all 512
   15.32 ± 0.30 times faster than prospero-memo-yes-vector-yes-sink-all 1024
   61.11 ± 1.04 times faster than prospero-memo-yes-vector-yes-sink-all 2048
  266.68 ± 4.33 times faster than prospero-memo-yes-vector-yes-sink-all 4096
 1095.86 ± 17.46 times faster than prospero-memo-yes-vector-yes-sink-all 8192
```

I am not sure I have a good explanation for that result.

### Intermediate representation metrics

The original `prospero.vm` has 7,866 instructions, while after the `simplify`
and `reassociate` passes 7,714 are left. That's less than a 2% reduction and yet
a ~20% improvement in runtime when using memoization. Here's the effect on the
number of VM instructions, per opcode:

|        | orig | simp | diff |
| ------ | ---: | ---: | ---: |
| add    | 1102 |  957 | -145 |
| const  | 1406 | 1229 | -177 |
| max    | 2094 | 2019 |  -75 |
| min    |  784 |  859 |  +75 |
| mul    |   36 |   33 |   -3 |
| neg    |  383 |  750 | +367 |
| sqrt   |  270 |  270 |    0 |
| square |  362 |  352 |  -10 |
| sub    | 1427 | 1243 | -184 |
| var-x  |    1 |    1 |    0 |
| var-y  |    1 |    1 |    0 |
| ------ | ---- | ---- | ---- |
| total  | 7866 | 7714 | -152 |

The reduction in `max` instructions is exactly matched by an increase in `min`
instructions; this is due to propagating negations through.

Ignoring constants, these passes actually increased the number of instructions,
by creating so many more negations than there were originally.

These passes reduce the number of intermediate results that must be stored by
memoization functions and then loaded by the main-loop. These figures determine
memory use. Each x output needs to be stored for every column, so for a
2048x2048 image there are 2048 copies of the set of x outputs. Similarly, each y
output needs as many copies as the vector size (e.g. 4).

| outputs           | orig | simp | diff  |
| -------           | ---: | ---: | ----: |
| const             | 1406 | 1229 |  -177 |
| x                 | 1025 |  695 |  -330 |
| x (KiB@2048px)    | 8200 | 5560 | -2640 |
| y                 |  598 |  545 |   -53 |
| y (KiB@4x vector) |    9 |    9 |    -1 |

Memoization is a win if enough instructions can be moved from the main-loop
to either of the x or y memoization functions. Reassociation helps move more
instructions out of the hot loop, as well as reducing the number of intermediate
values that need to be in memory.

| instructions | orig | simp | diff |
| ------------ | ---: | ---: | ---: |
| xy           | 5903 | 5279 | -624 |
| x            | 2405 | 2549 | +144 |
| y            | 1216 | 1204 |  -12 |
| ------------ | ---- | ---- | ---- |
| total        | 9524 | 9032 | -492 |

About 60% of the reduction in main-loop instructions is due to not needing to
load as many intermediate values from the memoization functions.

### x86 code size

The output of this compiler contains a constant pool and several functions. In
all of the above tests, the constant pool has one of three possible sizes:

- 0x13b8 bytes if not vectorized;
- 0x4dc0 bytes if vectorized, because there are four copies of every constant to
  allow them to be loaded as-is to fill a vector register; and
- 0x5870 bytes if vectorized and from the original program without simplification,
  since simplification unifies multiple copies of the same constant.

The sizes of the text sections of these binaries vary substantially, with
the largest being 58% bigger than the smallest.

| text size | simplify | memo | vector | sink loads |
| ------: | --- | --- | --- | --- |
|  0xb9c1 | yes | no  | yes | prefer-dead |
|  0xb9c1 | yes | no  | yes | spill-any |
|  0xbd01 | yes | no  | yes | require-dead |
|  0xc601 | yes | no  | yes | all |
|  0xcfb1 | no  | no  | yes | require-dead |
|  0xd291 | yes | no  | yes | none |
|  0xd421 | no  | no  | yes | all |
|  0xd821 | no  | no  | yes | none |
|  0xd8f1 | yes | no  | no  | none |
|  0xe801 | yes | yes | yes | prefer-dead |
|  0xe801 | yes | yes | yes | spill-any |
|  0xe831 | yes | yes | yes | require-dead |
|  0xf171 | yes | yes | yes | all |
| 0x10a11 | yes | yes | yes | none |
| 0x110a1 | no  | yes | yes | require-dead |
| 0x11211 | yes | yes | no  | none |
| 0x11501 | no  | yes | yes | all |
| 0x12581 | no  | yes | yes | none |

- The memoized versions all have more code than any of the unmemoized versions,
  because they need to store intermediate results to memory.
- Load sinking produces smaller code than not sinking loads, but loading into a
  register when one is free and otherwise sinking loads is even better in terms
  of code size.
- Applying the `simplify` and `reassociate` passes always results in smaller
  machine code, all else being equal, even though they produce more non-constant
  VM instructions.
- Vectorized code is smaller than unvectorized, which surprises me.

### x86 stack frame size

The size of the stack frame for a function is related to the worst-case register
pressure within that function, and to the register allocator's ability to
allocate the available registers efficiently. When many intermediate results are
live at the same time, some need to get spilled to memory.

I chose to let the register allocator use each element of output arrays as
scratch space until the final value of that element was written there. As a
result, when memoizing, the single-variable functions had plenty of scratch
space without allocating a stack frame. However, in every configuration, the
function which depends on both X and Y always allocated a stack frame.

| frame size | simplify | memo | vector | sink loads |
| ----: | --- | --- | --- | --- |
|  0x24 | yes | yes | no  | none |
|  0x80 | no  | yes | yes | all |
|  0x80 | no  | yes | yes | none |
|  0x80 | no  | yes | yes | require-dead |
|  0x90 | yes | yes | yes | all |
|  0x90 | yes | yes | yes | none |
|  0x90 | yes | yes | yes | prefer-dead |
|  0x90 | yes | yes | yes | require-dead |
|  0x90 | yes | yes | yes | spill-any |
| 0x1d8 | yes | no  | no  | none |
| 0x740 | yes | no  | yes | all |
| 0x740 | yes | no  | yes | require-dead |
| 0x750 | yes | no  | yes | prefer-dead |
| 0x750 | yes | no  | yes | spill-any |
| 0x750 | no  | no  | yes | all |
| 0x750 | no  | no  | yes | require-dead |
| 0x760 | yes | no  | yes | none |
| 0x770 | no  | no  | yes | none |

All of the memoized versions had significantly smaller stack frames, because
many of the intermediate results they needed were already in memory, written
there by the memoized single-variable functions.

Stack frames with vectorization were four times the size of stack frames
without, due to storing four-element vectors instead of individual scalar
values.

Other than those variations, all stack frame sizes were very similar across the
other choices of variables that I studied.

#### Loads and stores

Vectorization has no effect on these measures so I've omitted non-vectorized
cases from these tables.

Memoization generated more instructions that load from memory than almost all
cases without memoization, because it forces intermediate results into memory
that sometimes can stay in a register otherwise.

| loads | simplify | memo | vector | sink loads |
| ---: | --- | --- | --- | --- |
| 2421 | yes | no  | yes | prefer-dead |
| 2421 | yes | no  | yes | spill-any |
| 2500 | yes | no  | yes | none |
| 2608 | yes | no  | yes | require-dead |
| 2628 | no  | no  | yes | none |
| 3074 | no  | no  | yes | require-dead |
| 3194 | yes | yes | yes | prefer-dead |
| 3194 | yes | yes | yes | spill-any |
| 3235 | yes | yes | yes | none |
| 3320 | yes | yes | yes | require-dead |
| 3407 | yes | no  | yes | all |
| 3642 | no  | no  | yes | all |
| 3807 | no  | yes | yes | none |
| 4037 | no  | yes | yes | require-dead |
| 4094 | yes | yes | yes | all |
| 4572 | no  | yes | yes | all |

Similarly, memoization generated more instructions that store to memory than any
non-memoized case.

| stores | simplify | memo | vector | sink loads |
| ---: | --- | --- | --- | --- |
| 322  | yes | no  | yes | all |
| 327  | yes | no  | yes | require-dead |
| 330  | yes | no  | yes | prefer-dead |
| 330  | yes | no  | yes | spill-any |
| 334  | yes | no  | yes | none |
| 341  | no  | no  | yes | all |
| 346  | no  | no  | yes | require-dead |
| 361  | no  | no  | yes | none |
| 1352 | yes | yes | yes | all |
| 1359 | yes | yes | yes | require-dead |
| 1362 | yes | yes | yes | none |
| 1362 | yes | yes | yes | prefer-dead |
| 1362 | yes | yes | yes | spill-any |
| 1648 | no  | yes | yes | all |
| 1656 | no  | yes | yes | require-dead |
| 1661 | no  | yes | yes | none |

Note that the choice of load-sinking policy has an effect on not only the number
of loads but also the number of stores, because it affects how many values are
spilled.
