0000000000000018 <__ZN11patches_dsp6biquad10MonoBiquad4tick17hbfcd36731212e79eE>:
      18: 1e604001     	fmov	d1, d0
      1c: bd400002     	ldr	s2, [x0]
      20: 1e220800     	fmul	s0, s0, s2
      24: bd402803     	ldr	s3, [x0, #0x28]
      28: 1e232800     	fadd	s0, s0, s3
      2c: 1e604003     	fmov	d3, d0
      30: 340004a1     	cbz	w1, 0xc4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17hbfcd36731212e79eE+0xac>
      34: 1e2e1003     	fmov	s3, #1.00000000
      38: 1e209004     	fmov	s4, #2.50000000
      3c: 1e242000     	fcmp	s0, s4
      40: 5400042a     	b.ge	0xc4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17hbfcd36731212e79eE+0xac>
      44: 1e3e1003     	fmov	s3, #-1.00000000
      48: 1e309004     	fmov	s4, #-2.50000000
      4c: 1e242000     	fcmp	s0, s4
      50: 540003a9     	b.ls	0xc4 <__ZN11patches_dsp6biquad10MonoBiquad4tick17hbfcd36731212e79eE+0xac>
      54: 1e200803     	fmul	s3, s0, s0
      58: 1e230864     	fmul	s4, s3, s3
      5c: 1e240865     	fmul	s5, s3, s4
      60: 52900008     	mov	w8, #0x8000             ; =32768
      64: 72a893a8     	movk	w8, #0x449d, lsl #16
      68: 1e270106     	fmov	s6, w8
      6c: 1e260866     	fmul	s6, s3, s6
      70: 528d8008     	mov	w8, #0x6c00             ; =27648
      74: 72a8c448     	movk	w8, #0x4622, lsl #16
      78: 1e270107     	fmov	s7, w8
      7c: 1e2728c6     	fadd	s6, s6, s7
      80: 1e26b010     	fmov	s16, #21.00000000
      84: 1e300890     	fmul	s16, s4, s16
      88: 1e3028c6     	fadd	s6, s6, s16
      8c: 1e260806     	fmul	s6, s0, s6
      90: 52950008     	mov	w8, #0xa800             ; =43008
      94: 72a8b268     	movk	w8, #0x4593, lsl #16
      98: 1e270110     	fmov	s16, w8
      9c: 1e300863     	fmul	s3, s3, s16
      a0: 1e272863     	fadd	s3, s3, s7
      a4: 52a86a48     	mov	w8, #0x43520000         ; =1129447424
      a8: 1e270107     	fmov	s7, w8
      ac: 1e270884     	fmul	s4, s4, s7
      b0: 1e242863     	fadd	s3, s3, s4
      b4: 1e221004     	fmov	s4, #4.00000000
      b8: 1e2408a4     	fmul	s4, s5, s4
      bc: 1e242863     	fadd	s3, s3, s4
      c0: 1e2318c3     	fdiv	s3, s6, s3
      c4: 2d411005     	ldp	s5, s4, [x0, #0x8]
      c8: 1e240866     	fmul	s6, s3, s4
      cc: bd402c07     	ldr	s7, [x0, #0x2c]
      d0: bd401010     	ldr	s16, [x0, #0x10]
      d4: 1e300863     	fmul	s3, s3, s16
      d8: fc404011     	ldur	d17, [x0, #0x4]
      dc: 1e310832     	fmul	s18, s1, s17
      e0: 1e263a46     	fsub	s6, s18, s6
      e4: 1e2628e6     	fadd	s6, s7, s6
      e8: 1e250821     	fmul	s1, s1, s5
      ec: 1e233821     	fsub	s1, s1, s3
      f0: 2d050406     	stp	s6, s1, [x0, #0x28]
      f4: 3cc14001     	ldur	q1, [x0, #0x14]
      f8: 6e116003     	ext.16b	v3, v0, v17, #0xc
      fc: 6e040443     	mov.s	v3[0], v2[0]
     100: 6e1c0483     	mov.s	v3[3], v4[0]
     104: 4e21d461     	fadd.4s	v1, v3, v1
     108: 3d800001     	str	q1, [x0]
     10c: bd402401     	ldr	s1, [x0, #0x24]
     110: 1e212a01     	fadd	s1, s16, s1
     114: bd001001     	str	s1, [x0, #0x10]
     118: d65f03c0     	ret

000000000000011c <__ZN11patches_dsp6biquad10PolyBiquad10new_static17h0b0a3b770fe01694E>:
     11c: 4e040400     	dup.4s	v0, v0[0]
     120: ad000100     	stp	q0, q0, [x8]
     124: ad010100     	stp	q0, q0, [x8, #0x20]
     128: 4e040421     	dup.4s	v1, v1[0]
     12c: ad020501     	stp	q1, q1, [x8, #0x40]
     130: ad030501     	stp	q1, q1, [x8, #0x60]
     134: 4e040442     	dup.4s	v2, v2[0]
     138: ad040902     	stp	q2, q2, [x8, #0x80]
     13c: ad050902     	stp	q2, q2, [x8, #0xa0]
     140: 4e040463     	dup.4s	v3, v3[0]
     144: ad060d03     	stp	q3, q3, [x8, #0xc0]
     148: ad070d03     	stp	q3, q3, [x8, #0xe0]
     14c: 4e040484     	dup.4s	v4, v4[0]
     150: ad081104     	stp	q4, q4, [x8, #0x100]
     154: ad091104     	stp	q4, q4, [x8, #0x120]
     158: 6f00e405     	movi.2d	v5, #0000000000000000
     15c: ad131505     	stp	q5, q5, [x8, #0x260]
     160: ad121505     	stp	q5, q5, [x8, #0x240]
     164: ad111505     	stp	q5, q5, [x8, #0x220]
     168: ad101505     	stp	q5, q5, [x8, #0x200]
     16c: ad0f1505     	stp	q5, q5, [x8, #0x1e0]
     170: ad0e1505     	stp	q5, q5, [x8, #0x1c0]
     174: ad0d1505     	stp	q5, q5, [x8, #0x1a0]
     178: ad0c1505     	stp	q5, q5, [x8, #0x180]
     17c: ad0b1505     	stp	q5, q5, [x8, #0x160]
     180: ad0a1505     	stp	q5, q5, [x8, #0x140]
     184: ad171505     	stp	q5, q5, [x8, #0x2e0]
     188: ad161505     	stp	q5, q5, [x8, #0x2c0]
     18c: ad180100     	stp	q0, q0, [x8, #0x300]
     190: ad190100     	stp	q0, q0, [x8, #0x320]
     194: ad1a0501     	stp	q1, q1, [x8, #0x340]
     198: ad1b0501     	stp	q1, q1, [x8, #0x360]
     19c: ad1c0902     	stp	q2, q2, [x8, #0x380]
     1a0: ad1d0902     	stp	q2, q2, [x8, #0x3a0]
     1a4: ad1e0d03     	stp	q3, q3, [x8, #0x3c0]
     1a8: ad1f0d03     	stp	q3, q3, [x8, #0x3e0]
     1ac: 3d810104     	str	q4, [x8, #0x400]
     1b0: 3d810504     	str	q4, [x8, #0x410]
     1b4: 3d810904     	str	q4, [x8, #0x420]
     1b8: 3d810d04     	str	q4, [x8, #0x430]
     1bc: ad151505     	stp	q5, q5, [x8, #0x2a0]
     1c0: ad141505     	stp	q5, q5, [x8, #0x280]
     1c4: 3911011f     	strb	wzr, [x8, #0x440]
     1c8: d65f03c0     	ret

00000000000001cc <__ZN11patches_dsp6biquad10PolyBiquad10set_static17h0f43c405663547f0E>:
     1cc: 3911001f     	strb	wzr, [x0, #0x440]
     1d0: 4e040400     	dup.4s	v0, v0[0]
     1d4: ad000000     	stp	q0, q0, [x0]
     1d8: ad010000     	stp	q0, q0, [x0, #0x20]
     1dc: 4e040421     	dup.4s	v1, v1[0]
     1e0: ad020401     	stp	q1, q1, [x0, #0x40]
     1e4: ad030401     	stp	q1, q1, [x0, #0x60]
     1e8: 4e040442     	dup.4s	v2, v2[0]
     1ec: ad040802     	stp	q2, q2, [x0, #0x80]
     1f0: ad050802     	stp	q2, q2, [x0, #0xa0]
     1f4: 4e040463     	dup.4s	v3, v3[0]
     1f8: ad060c03     	stp	q3, q3, [x0, #0xc0]
     1fc: ad070c03     	stp	q3, q3, [x0, #0xe0]
     200: 4e040484     	dup.4s	v4, v4[0]
     204: ad081004     	stp	q4, q4, [x0, #0x100]
     208: ad091004     	stp	q4, q4, [x0, #0x120]
     20c: 6f00e405     	movi.2d	v5, #0000000000000000
     210: ad131405     	stp	q5, q5, [x0, #0x260]
     214: ad121405     	stp	q5, q5, [x0, #0x240]
     218: ad111405     	stp	q5, q5, [x0, #0x220]
     21c: ad101405     	stp	q5, q5, [x0, #0x200]
     220: ad0f1405     	stp	q5, q5, [x0, #0x1e0]
     224: ad0e1405     	stp	q5, q5, [x0, #0x1c0]
     228: ad0d1405     	stp	q5, q5, [x0, #0x1a0]
     22c: ad0c1405     	stp	q5, q5, [x0, #0x180]
     230: ad0b1405     	stp	q5, q5, [x0, #0x160]
     234: ad0a1405     	stp	q5, q5, [x0, #0x140]
     238: ad180000     	stp	q0, q0, [x0, #0x300]
     23c: ad190000     	stp	q0, q0, [x0, #0x320]
     240: ad1a0401     	stp	q1, q1, [x0, #0x340]
     244: ad1b0401     	stp	q1, q1, [x0, #0x360]
     248: ad1c0802     	stp	q2, q2, [x0, #0x380]
     24c: ad1d0802     	stp	q2, q2, [x0, #0x3a0]
     250: ad1e0c03     	stp	q3, q3, [x0, #0x3c0]
     254: ad1f0c03     	stp	q3, q3, [x0, #0x3e0]
     258: 3d810004     	str	q4, [x0, #0x400]
     25c: 3d810404     	str	q4, [x0, #0x410]
     260: 3d810804     	str	q4, [x0, #0x420]
     264: 3d810c04     	str	q4, [x0, #0x430]
     268: d65f03c0     	ret

