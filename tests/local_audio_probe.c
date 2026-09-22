/* Host acceptance probe for the production C decoder; generates no network traffic. */
#include "local_audio.h"
#include <assert.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <libavformat/avformat.h>
#include <libavutil/mathematics.h>
static int64_t samples,first=INT64_MIN,last=INT64_MIN;
static int peak;
static int receive(void *d,FILE *out) {
    for(int i=0;i<4096;++i) {
        const uint8_t *data=NULL;int bytes=0;int64_t pts=0;
        int rc=plx_audio_receive(d,&data,&bytes,&pts);
        if(rc==0 || rc==2)return rc;
        assert(rc==1 && data && bytes>0 && bytes<=65536*4 && bytes%4==0);
        if(first==INT64_MIN)first=pts;
        assert(last==INT64_MIN || pts>=last);last=pts;
        for(int j=0;j<bytes;j+=2) {
            int value=(int16_t)((unsigned)data[j]|((unsigned)data[j+1]<<8));
            if(abs(value)>peak)peak=abs(value);
        }
        if(out)assert(fwrite(data,1,(size_t)bytes,out)==(size_t)bytes);
        samples+=bytes/4;
    }
    abort();
}
int main(int argc,char **argv) {
    assert(argc==6 || argc==7);
    unsigned mask=plx_audio_init(argv[1],argv[2],argv[3]);assert(mask==31);
    assert(plx_audio_decoder_open(NULL)==NULL);
    assert(plx_audio_send(NULL,NULL,0,0)==-1);
    AVFormatContext *fmt=NULL;
    assert(avformat_open_input(&fmt,argv[4],NULL,NULL)==0);
    assert(avformat_find_stream_info(fmt,NULL)>=0);
    int ai=av_find_best_stream(fmt,AVMEDIA_TYPE_AUDIO,-1,-1,NULL,0);assert(ai>=0);
    AVStream *st=fmt->streams[ai];
    void *d=plx_audio_decoder_open(st->codecpar);assert(d);
    uint8_t byte=0;
    assert(plx_audio_send(d,&byte,4*1024*1024+1,0)==-1);
    assert(plx_audio_send(d,&byte,-1,0)==-1);
    assert(plx_audio_send(d,&byte,1,INT64_MAX)==-1);
    FILE *out=fopen(argv[5],"wb");assert(out);
    AVPacket *pkt=av_packet_alloc();assert(pkt);
    for(int pass=0;pass<(argc==7?2:1);++pass) {
        if(pass) {
            assert(av_seek_frame(fmt,ai,0,AVSEEK_FLAG_BACKWARD)>=0);
            plx_audio_decoder_flush(d);
            samples=0;first=last=INT64_MIN;peak=0;
        }
        int rc;
        while((rc=av_read_frame(fmt,pkt))>=0) {
            if(pkt->stream_index==ai) {
                int64_t pts=pkt->pts!=AV_NOPTS_VALUE?pkt->pts:pkt->dts;
                if(pts==AV_NOPTS_VALUE)pts=0;
                pts=av_rescale_q(pts,st->time_base,(AVRational){1,1000000000});
                int64_t duration=av_rescale_q(pkt->duration,st->time_base,(AVRational){1,1000000000});
                int sent=plx_audio_send_packet(d,pkt,pts,duration);
                if(sent==0) {receive(d,out);sent=plx_audio_send_packet(d,pkt,pts,duration);}
                assert(sent==1);receive(d,out);
            }
            av_packet_unref(pkt);
        }
        assert(rc==AVERROR_EOF);
        int sent=plx_audio_send(d,NULL,0,0);
        if(sent==0){receive(d,out);sent=plx_audio_send(d,NULL,0,0);}
        assert(sent==1);assert(receive(d,out)==2);
    }
    printf("{\"samples\":%"PRId64",\"first_ns\":%"PRId64",\"last_ns\":%"PRId64",\"peak\":%d}\n",samples,first,last,peak);
    assert(samples>44000 && samples<52000 && peak>100 && peak<=32768);
    fclose(out);av_packet_free(&pkt);plx_audio_decoder_close(d);avformat_close_input(&fmt);
    return 0;
}
