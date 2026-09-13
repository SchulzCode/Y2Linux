# Storage02 rescue console with SD present

The owner photo shows display/DSI diagnostics, then at approximately 41.66 seconds:

```
Y2RESCUE: missing/invalid internal Y2ROOT or Y2DATA; no reboot, no SD fallback, ACM retained
sh: can't access tty; job control turned off
~ #
```

This confirms Linux/rescue execution and the root resolver's failure path. Normal
Buildroot switch_root did not occur. The retained-ACM text is intent, not proof
that USB enumerated. The prompt does not itself establish a usable input console.
The earlier empty-SD warning traffic is absent from this screen with SD present.
Neither root-versus-data rejection nor internal card initialization is identified
by the aggregate error, so actual block/driver logs are required before a fix.

Source review identifies a separate startup prerequisite: kernel/usb/y2_musb.c
function y2_usb_begin rejects when initial CHRDET is asserted. The established
AUDIO-02 deployment says disconnect after flashing, boot unplugged, then attach
once after startup. The assistant's instruction to keep USB connected during
restart was wrong for this existing driver. Correct the production instructions;
do not claim this proves the exact device-side USB result without logs.

Next read-only observation: owner restart unplugged, attach once after at least
ten seconds, capture the existing rescue LOG1 ACM stream. No eMMC write or
speculative firmware change is required to try this established startup path.

Private photo SHA256: 35dea75e0fd423b6185b1bb337c0e8d56a80b08483556e21a229fea87fe63ea4
