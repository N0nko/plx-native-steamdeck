/* Optional software audio boundary. No FFmpeg structure crosses by value. */
#ifndef PLX_LOCAL_AUDIO_H
#define PLX_LOCAL_AUDIO_H
#include <stdint.h>
#include <stddef.h>
/* Bits: DTS=1, TrueHD=2, FLAC=4, Opus=8, Vorbis=16. Main thread, once at boot. */
unsigned plx_audio_init(const char *util, const char *codec, const char *resample);
void *plx_audio_decoder_open(const void *codec_parameters);
/* send: 1 accepted, 0 drain receive before retry, -1 failure. NULL/0 drains EOF. */
int plx_audio_send(void *, const uint8_t *, int bytes, int64_t pts_ns);
/* Production demux uses packet_ref to preserve skip-samples/discard-padding side data. */
int plx_audio_send_packet(void *, const void *packet, int64_t pts_ns, int64_t duration_ns);
/* receive: 1 frame, 0 need input, 2 EOF, -1 failure. Buffer valid until next call. */
int plx_audio_receive(void *, const uint8_t **, int *bytes, int64_t *pts_ns);
void plx_audio_decoder_flush(void *);
void plx_audio_decoder_close(void *);
/* PCM sink is main-thread-confined, stereo S16LE at 48 kHz. Never blocks on playback. */
void *plx_pcm_open(void);
void plx_pcm_close(void *);
void plx_pcm_pause(void *, int paused);
void plx_pcm_flush(void *);
unsigned plx_pcm_queued(void *);
/* 1 consumed (including stale data), 0 retry later, -1 output failure. */
int plx_pcm_feed(void *, const uint8_t *, int bytes, int64_t pts_ns, int64_t video_ns, int priming);
#endif
