# Hardware Final USB DMA observations

The sole active peripheral adapter is `kernel/platform/usb.c`. Hardware 02's
physical census on 2026-09-27 reports Inventra DMA active, advancing DMA IRQs
and zero DMA errors. This establishes activity, not the throughput ceiling,
channel utilization, or suspend qualification.

Hardware Final adds read-only fields to the existing
`/sys/devices/platform/11200000.usb/status` interface:

| Fields | Meaning |
| --- | --- |
| `transfer`, `dma_irqs`, `dma_errors` | Existing controller mode, aggregate DMA IRQs and detected bus faults |
| `dma_allocations`, `dma_alloc_failures`, `dma_releases` | Successful upstream channel allocations, allocation refusals and normal release calls |
| `dma_allocated` | Currently allocated channels; allocation does not imply an in-flight transfer |
| `dma_rx_programs`, `dma_tx_programs` | Accepted upstream channel-program calls, from the device's perspective |
| `dma_rx_programmed_bytes`, `dma_tx_programmed_bytes` | Bytes in those accepted programming requests; **not completed bytes, wire traffic or TCP goodput** |
| `dma_program_failures` | Upstream programming refusals; normal PIO fallback may follow |
| `dma_aborts`, `dma_abort_failures` | Abort calls and their nonzero returns; an idle-channel abort is still a call |

Totals span the driver's lifetime. Teardown resets the current allocation gauge;
upstream emergency controller stop may release remaining channels directly, so
allocation minus normal releases is not a substitute for the gauge. RX/TX
classification follows allocation direction, independently of DMA mode 0/1.
Atomic 64-bit totals avoid torn reads and short byte-counter wrap on ARM32.
Each field is individually coherent; the whole multiline report is not an
atomic snapshot of ongoing traffic. Use bounded before/after deltas.

The wrappers delegate allocation, programming, abort and release unchanged.
Allocation/program failure remains the upstream result. Existing
`y2.usb_dma=off`, allocation fallback, 32-bit DMA addressing, peripheral-only
role and terminal bus-fault teardown remain in force. Throughput and CPU cost
must be remeasured on the final candidate, including instrumentation cost.

The existing IRQ-overflow snapshot retains sampled L1/core/endpoint status and
masks before teardown. The Physical 01 suspend failure alone does not prove a
particular stuck endpoint, bad restore ordering, or a DMA defect (that image
used PIO). Hardware 02 introduced the snapshot; a repeat failure's retained
snapshot is required before attributing its root cause. The current 512-IRQs
per-jiffy guard remains unchanged. High IRQ rate is not independently proof of
an electrical fault; retain throughput, elapsed time and status when it fires.

Targeted host validation extracts and executes the actual wrappers, covering
successful delegation, allocation exhaustion/reuse, rejected programming,
RX/TX direction independent of mode, byte totals crossing 4 GiB, and abort
failure propagation. The existing USB lifecycle/recovery tests and DMA
bus-error teardown test also pass. These checks do not certify physical
throughput, reconnect, suspend or fault recovery.
