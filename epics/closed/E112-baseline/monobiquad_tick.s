0000000000000a48 <__ZN11patches_dsp6biquad10MonoBiquad4tick17h52bad5c8dd3eead1E>:
     a48: 1e604001     	fmov	d1, d0
     a4c: bd400002     	ldr	s2, [x0]
     a50: 1e220800     	fmul	s0, s0, s2
     a54: bd403c03     	ldr	s3, [x0, #0x3c]
     a58: 1e232800     	fadd	s0, s0, s3
     a5c: 1e604003     	fmov	d3, d0
     a60: 340004a1     	cbz	w1, 0xaf4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17h52bad5c8dd3eead1E+0xac>
     a64: 1e2e1003     	fmov	s3, #1.00000000
     a68: 1e209004     	fmov	s4, #2.50000000
     a6c: 1e242000     	fcmp	s0, s4
     a70: 5400042a     	b.ge	0xaf4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17h52bad5c8dd3eead1E+0xac>
     a74: 1e3e1003     	fmov	s3, #-1.00000000
     a78: 1e309004     	fmov	s4, #-2.50000000
     a7c: 1e242000     	fcmp	s0, s4
     a80: 540003a9     	b.ls	0xaf4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17h52bad5c8dd3eead1E+0xac>
     a84: 1e200803     	fmul	s3, s0, s0
     a88: 1e230864     	fmul	s4, s3, s3
     a8c: 1e240865     	fmul	s5, s3, s4
     a90: 52900008     	mov	w8, #0x8000             ; =32768
     a94: 72a893a8     	movk	w8, #0x449d, lsl #16
     a98: 1e270106     	fmov	s6, w8
     a9c: 1e260866     	fmul	s6, s3, s6
     aa0: 528d8008     	mov	w8, #0x6c00             ; =27648
     aa4: 72a8c448     	movk	w8, #0x4622, lsl #16
     aa8: 1e270107     	fmov	s7, w8
     aac: 1e2728c6     	fadd	s6, s6, s7
     ab0: 1e26b010     	fmov	s16, #21.00000000
     ab4: 1e300890     	fmul	s16, s4, s16
     ab8: 1e3028c6     	fadd	s6, s6, s16
     abc: 1e260806     	fmul	s6, s0, s6
     ac0: 52950008     	mov	w8, #0xa800             ; =43008
     ac4: 72a8b268     	movk	w8, #0x4593, lsl #16
     ac8: 1e270110     	fmov	s16, w8
     acc: 1e300863     	fmul	s3, s3, s16
     ad0: 1e272863     	fadd	s3, s3, s7
     ad4: 52a86a48     	mov	w8, #0x43520000         ; =1129447424
     ad8: 1e270107     	fmov	s7, w8
     adc: 1e270884     	fmul	s4, s4, s7
     ae0: 1e242863     	fadd	s3, s3, s4
     ae4: 1e221004     	fmov	s4, #4.00000000
     ae8: 1e2408a4     	fmul	s4, s5, s4
     aec: 1e242863     	fadd	s3, s3, s4
     af0: 1e2318c3     	fdiv	s3, s6, s3
     af4: 2d411005     	ldp	s5, s4, [x0, #0x8]
     af8: 1e240866     	fmul	s6, s3, s4
     afc: bd404007     	ldr	s7, [x0, #0x40]
     b00: bd401010     	ldr	s16, [x0, #0x10]
     b04: 1e300863     	fmul	s3, s3, s16
     b08: fc404011     	ldur	d17, [x0, #0x4]
     b0c: 1e310832     	fmul	s18, s1, s17
     b10: 1e263a46     	fsub	s6, s18, s6
     b14: 1e2628e6     	fadd	s6, s7, s6
     b18: 1e250821     	fmul	s1, s1, s5
     b1c: 1e233821     	fsub	s1, s1, s3
     b20: 2d078406     	stp	s6, s1, [x0, #0x3c]
     b24: 3cc28001     	ldur	q1, [x0, #0x28]
     b28: 6e116003     	ext.16b	v3, v0, v17, #0xc
     b2c: 6e040443     	mov.s	v3[0], v2[0]
     b30: 6e1c0483     	mov.s	v3[3], v4[0]
     b34: 4e21d461     	fadd.4s	v1, v3, v1
     b38: 3d800001     	str	q1, [x0]
     b3c: bd403801     	ldr	s1, [x0, #0x38]
     b40: 1e212a01     	fadd	s1, s16, s1
     b44: bd001001     	str	s1, [x0, #0x10]
     b48: d65f03c0     	ret

