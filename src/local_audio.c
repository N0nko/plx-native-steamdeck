/* FFmpeg 9 audio decoding. Optional, LGPL-compatible native decoders only.
 * Output is bounded stereo PCM; no claim of Atmos/DTS:X rendering or passthrough.
 * All pointers are resolved from the app's exact FFmpeg libraries, never LG's copy. */
#include "local_audio.h"
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#ifndef PLX_LOCAL_AUDIO
#define PLX_LOCAL_AUDIO 0
#endif
#if PLX_LOCAL_AUDIO
#include <dlfcn.h>
#include <errno.h>
#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/mathematics.h>
#include <libswresample/swresample.h>
_Static_assert(LIBAVCODEC_VERSION_MAJOR == 63, "local audio requires the bundled FFmpeg 9 headers");
_Static_assert(LIBAVUTIL_VERSION_MAJOR == 61, "avutil ABI");
_Static_assert(LIBSWRESAMPLE_VERSION_MAJOR == 7, "swresample ABI");
#define FN(name) static __typeof__(&name) p_##name
FN(avcodec_version); FN(avutil_version); FN(swresample_version);
FN(avcodec_find_decoder); FN(avcodec_alloc_context3); FN(avcodec_parameters_to_context);
FN(avcodec_open2); FN(avcodec_free_context); FN(avcodec_send_packet); FN(avcodec_receive_frame);
FN(avcodec_flush_buffers); FN(av_packet_ref); FN(av_packet_alloc); FN(av_new_packet); FN(av_packet_unref); FN(av_packet_free);
FN(av_frame_alloc); FN(av_frame_free); FN(av_frame_unref); FN(av_rescale_q); FN(av_rescale_rnd);
FN(av_channel_layout_copy); FN(av_channel_layout_uninit); FN(av_channel_layout_compare);
FN(swr_alloc_set_opts2); FN(swr_init); FN(swr_free); FN(swr_get_delay); FN(swr_convert);
static void *libs[3];
static unsigned ready_mask;
static int attempted;
static const AVRational ns_tb = {1, 1000000000};
static const AVRational out_tb = {1, 48000};
static unsigned codec_bit(enum AVCodecID id) {
    switch (id) {
    case AV_CODEC_ID_DTS: return 1;
    case AV_CODEC_ID_TRUEHD: return 2;
    case AV_CODEC_ID_FLAC: return 4;
    case AV_CODEC_ID_OPUS: return 8;
    case AV_CODEC_ID_VORBIS: return 16;
    default: return 0;
    }
}
unsigned plx_audio_init(const char *util, const char *codec, const char *resample) {
    if (attempted) return ready_mask;
    attempted = 1;
    if (!util || !codec || !resample || util[0] != '/' || codec[0] != '/' || resample[0] != '/') return 0;
    const char *paths[] = {util, codec, resample};
    for (int i = 0; i < 3; ++i) if (!(libs[i] = dlopen(paths[i], RTLD_NOW | RTLD_LOCAL))) goto fail;
#define LOAD(lib, name) do { *(void **)(&p_##name) = dlsym(libs[lib], #name); if (!p_##name) goto fail; } while (0)
    LOAD(1,avcodec_version); LOAD(0,avutil_version); LOAD(2,swresample_version);
    if ((p_avcodec_version() >> 16) != 63 || (p_avutil_version() >> 16) != 61 ||
        (p_swresample_version() >> 16) != 7) goto fail;
    LOAD(1,avcodec_find_decoder); LOAD(1,avcodec_alloc_context3); LOAD(1,avcodec_parameters_to_context);
    LOAD(1,avcodec_open2); LOAD(1,avcodec_free_context); LOAD(1,avcodec_send_packet); LOAD(1,avcodec_receive_frame);
    LOAD(1,avcodec_flush_buffers); LOAD(1,av_packet_ref); LOAD(1,av_packet_alloc); LOAD(1,av_new_packet); LOAD(1,av_packet_unref); LOAD(1,av_packet_free);
    LOAD(0,av_frame_alloc); LOAD(0,av_frame_free); LOAD(0,av_frame_unref); LOAD(0,av_rescale_q); LOAD(0,av_rescale_rnd);
    LOAD(0,av_channel_layout_copy); LOAD(0,av_channel_layout_uninit); LOAD(0,av_channel_layout_compare);
    LOAD(2,swr_alloc_set_opts2); LOAD(2,swr_init); LOAD(2,swr_free); LOAD(2,swr_get_delay); LOAD(2,swr_convert);
#undef LOAD
    const enum AVCodecID ids[] = {AV_CODEC_ID_DTS, AV_CODEC_ID_TRUEHD, AV_CODEC_ID_FLAC, AV_CODEC_ID_OPUS, AV_CODEC_ID_VORBIS};
    for (unsigned i = 0; i < sizeof(ids)/sizeof(ids[0]); ++i)
        if (p_avcodec_find_decoder(ids[i])) ready_mask |= codec_bit(ids[i]);
    return ready_mask;
fail:
    for (int i = 2; i >= 0; --i) if (libs[i]) { dlclose(libs[i]); libs[i] = NULL; }
    return 0;
}
struct Decoder {
    AVCodecContext *codec;
    AVPacket *packet;
    AVFrame *frame;
    SwrContext *swr;
    AVChannelLayout layout;
    enum AVSampleFormat format;
    int rate, eof;
    uint8_t *pcm;
    int64_t anchor_ns, out_samples;
    int have_clock;
};
void plx_audio_decoder_close(void *opaque) {
    struct Decoder *d = opaque;
    if (!d) return;
    if (d->codec) p_avcodec_free_context(&d->codec);
    if (d->packet) p_av_packet_free(&d->packet);
    if (d->frame) p_av_frame_free(&d->frame);
    if (d->swr) p_swr_free(&d->swr);
    p_av_channel_layout_uninit(&d->layout);
    free(d->pcm); free(d);
}
void *plx_audio_decoder_open(const void *parameters) {
    const AVCodecParameters *par = parameters;
    if (!ready_mask || !par || par->codec_type != AVMEDIA_TYPE_AUDIO ||
        !(codec_bit(par->codec_id) & ready_mask) || par->extradata_size < 0 ||
        par->extradata_size > 256 * 1024 || par->ch_layout.nb_channels < 1 || par->ch_layout.nb_channels > 8 ||
        par->sample_rate < 8000 || par->sample_rate > 192000) return NULL;
    const AVCodec *codec = p_avcodec_find_decoder(par->codec_id);
    if (!codec) return NULL;
    struct Decoder *d = calloc(1,sizeof(*d));
    if (!d) return NULL;
    d->codec = p_avcodec_alloc_context3(codec);
    d->packet = p_av_packet_alloc(); d->frame = p_av_frame_alloc();
    d->pcm = malloc(65536u * 4u);
    if (!d->codec || !d->packet || !d->frame || !d->pcm ||
        p_avcodec_parameters_to_context(d->codec,par) < 0) goto fail;
    d->codec->pkt_timebase = ns_tb;
    d->codec->thread_count = 1;
    if (p_avcodec_open2(d->codec,codec,NULL) < 0) goto fail;
    return d;
fail: plx_audio_decoder_close(d); return NULL;
}
int plx_audio_send(void *opaque, const uint8_t *data, int bytes, int64_t pts_ns) {
    struct Decoder *d = opaque;
    if (!d || bytes < 0 || bytes > 4*1024*1024 || (!data && bytes) ||
        pts_ns < -3600LL*1000000000LL || pts_ns > 604800LL*1000000000LL) return -1;
    int r;
    if (!data && !bytes) r = p_avcodec_send_packet(d->codec,NULL);
    else {
        if (!bytes) return -1;
        p_av_packet_unref(d->packet);
        /* av_new_packet allocates AND zeros FFmpeg's mandatory decoder padding. */
        if (p_av_new_packet(d->packet,bytes) < 0) return -1;
        memcpy(d->packet->data,data,(size_t)bytes);
        d->packet->pts = pts_ns; d->packet->dts = AV_NOPTS_VALUE;
        r = p_avcodec_send_packet(d->codec,d->packet);
        p_av_packet_unref(d->packet);
    }
    return r == AVERROR(EAGAIN) ? 0 : (r < 0 && r != AVERROR_EOF ? -1 : 1);
}
int plx_audio_send_packet(void *opaque, const void *packet, int64_t pts_ns, int64_t duration_ns) {
    struct Decoder *d=opaque; const AVPacket *src=packet;
    if (!d || !src || src->size<0 || src->size>4*1024*1024 || !src->data ||
        pts_ns < -3600LL*1000000000LL || pts_ns > 604800LL*1000000000LL ||
        duration_ns<0 || duration_ns>3600LL*1000000000LL) return -1;
    p_av_packet_unref(d->packet);
    if(p_av_packet_ref(d->packet,src)<0) return -1;
    d->packet->pts=pts_ns; d->packet->dts=AV_NOPTS_VALUE;
    d->packet->duration=duration_ns; d->packet->time_base=ns_tb;
    int rc=p_avcodec_send_packet(d->codec,d->packet);
    p_av_packet_unref(d->packet);
    return rc==AVERROR(EAGAIN)?0:(rc<0?-1:1);
}
void plx_audio_decoder_flush(void *opaque) {
    struct Decoder *d = opaque; if (!d) return;
    p_avcodec_flush_buffers(d->codec); p_av_frame_unref(d->frame);
    p_av_packet_unref(d->packet);
    p_swr_free(&d->swr); p_av_channel_layout_uninit(&d->layout);
    d->rate = 0; d->have_clock = 0; d->out_samples = 0; d->eof = 0;
}
static int emit(struct Decoder *d, int samples, const uint8_t **data, int *bytes, int64_t *pts) {
    if (samples < 0 || samples > 65536 || d->out_samples > 604800LL*48000) return -1;
    if (!samples) return 0;
    int64_t delta = p_av_rescale_q(d->out_samples,out_tb,ns_tb);
    if (d->anchor_ns > INT64_MAX-delta || d->out_samples > INT64_MAX-samples) return -1;
    *pts = d->anchor_ns + delta; *bytes = samples * 4; *data = d->pcm;
    d->out_samples += samples; return 1;
}
int plx_audio_receive(void *opaque, const uint8_t **data, int *bytes, int64_t *pts) {
    struct Decoder *d = opaque;
    if (!d || !data || !bytes || !pts) return -1;
    *data = NULL; *bytes = 0;
    for (int tries=0; tries<64; ++tries) {
        if (d->eof) {
            if (!d->swr) return 2;
            uint8_t *out[] = {d->pcm};
            int n = p_swr_convert(d->swr,out,65536,NULL,0);
            return n == 0 ? 2 : emit(d,n,data,bytes,pts);
        }
        p_av_frame_unref(d->frame);
        int r = p_avcodec_receive_frame(d->codec,d->frame);
        if (r == AVERROR(EAGAIN)) return 0;
        if (r == AVERROR_EOF) { d->eof=1; continue; }
        if (r < 0) return -1;
        AVFrame *f = d->frame;
        if (f->sample_rate < 8000 || f->sample_rate > 192000 || f->nb_samples < 0 ||
            f->nb_samples > 65536 || f->ch_layout.nb_channels < 1 || f->ch_layout.nb_channels > 8) return -1;
        if (!d->swr || d->rate != f->sample_rate || d->format != f->format ||
            p_av_channel_layout_compare(&d->layout,&f->ch_layout)) {
            /* Reject mid-stream format changes rather than dropping a buffered resampler tail. */
            if (d->swr) return -1;
            AVChannelLayout stereo = AV_CHANNEL_LAYOUT_STEREO;
            if (p_swr_alloc_set_opts2(&d->swr,&stereo,AV_SAMPLE_FMT_S16,48000,
                    &f->ch_layout,(enum AVSampleFormat)f->format,f->sample_rate,0,NULL) < 0 ||
                p_swr_init(d->swr) < 0 || p_av_channel_layout_copy(&d->layout,&f->ch_layout) < 0) return -1;
            d->rate=f->sample_rate; d->format=(enum AVSampleFormat)f->format;
        }
        if (!d->have_clock) {
            /* A clockless first frame cannot be scheduled truthfully against video. */
            if (f->best_effort_timestamp == AV_NOPTS_VALUE) return -1;
            d->anchor_ns = f->best_effort_timestamp;
            d->have_clock=1;
        } else if (f->best_effort_timestamp != AV_NOPTS_VALUE) {
            int64_t predicted = d->anchor_ns + p_av_rescale_q(d->out_samples,out_tb,ns_tb)
                + p_av_rescale_q(p_swr_get_delay(d->swr,d->rate),(AVRational){1,d->rate},ns_tb);
            /* Preserve real gaps. No stale resampler data may straddle a discontinuity. */
            if (f->best_effort_timestamp > predicted + 250000000LL ||
                f->best_effort_timestamp < predicted - 250000000LL) {
                p_swr_free(&d->swr); p_av_channel_layout_uninit(&d->layout);
                AVChannelLayout stereo=AV_CHANNEL_LAYOUT_STEREO;
                if (p_swr_alloc_set_opts2(&d->swr,&stereo,AV_SAMPLE_FMT_S16,48000,
                    &f->ch_layout,(enum AVSampleFormat)f->format,f->sample_rate,0,NULL)<0 ||
                    p_swr_init(d->swr)<0 || p_av_channel_layout_copy(&d->layout,&f->ch_layout)<0) return -1;
                d->anchor_ns=f->best_effort_timestamp; d->out_samples=0;
            }
        }
        int64_t capacity=p_av_rescale_rnd(p_swr_get_delay(d->swr,d->rate)+f->nb_samples,
                                        48000,d->rate,AV_ROUND_UP);
        if (capacity<0 || capacity>65536) return -1;
        uint8_t *out[]={d->pcm};
        int n=p_swr_convert(d->swr,out,(int)capacity,(const uint8_t **)f->extended_data,f->nb_samples);
        int result=emit(d,n,data,bytes,pts); if (result) return result;
    }
    return -1; /* bounded malformed/empty-frame stream */
}
#else
unsigned plx_audio_init(const char *a,const char *b,const char *c) {(void)a;(void)b;(void)c;return 0;}
void *plx_audio_decoder_open(const void *p) {(void)p;return NULL;}
int plx_audio_send(void *p,const uint8_t *d,int n,int64_t t) {(void)p;(void)d;(void)n;(void)t;return -1;}
int plx_audio_receive(void *p,const uint8_t **d,int *n,int64_t *t) {(void)p;(void)d;(void)n;(void)t;return -1;}
int plx_audio_send_packet(void *p,const void *s,int64_t t,int64_t n) {(void)p;(void)s;(void)t;(void)n;return -1;}
void plx_audio_decoder_flush(void *p) {(void)p;}
void plx_audio_decoder_close(void *p) {(void)p;}
#endif
