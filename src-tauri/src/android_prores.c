#include <errno.h>
#include <math.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/opt.h>

#define EXPORT __attribute__((visibility("default")))

typedef struct {
    AVFormatContext *format;
    AVCodecContext *codec;
    AVStream *video;
    AVStream *audio;
    AVFrame *frame;
    AVPacket *packet;
    int64_t video_count;
    int64_t audio_count;
    int alpha;
    int transfer;
    float *pq;
} ProRes;

EXPORT void phitkc_prores_close(ProRes *writer) {
    if (!writer)
        return;
    av_packet_free(&writer->packet);
    av_frame_free(&writer->frame);
    avcodec_free_context(&writer->codec);
    if (writer->format) {
        avio_closep(&writer->format->pb);
        avformat_free_context(writer->format);
    }
    free(writer->pq);
    free(writer);
}

static float pq_encode(float value) {
    float v = powf(fminf(fmaxf(value / 100.0f, 0.0f), 1.0f), 0.15930176f);
    return powf((0.8359375f + 18.851563f * v) / (1.0f + 18.6875f * v), 78.84375f);
}

static float half_float(uint16_t bits) {
    unsigned exponent = (bits >> 10) & 31;
    unsigned fraction = bits & 1023;
    if (exponent == 31)
        return 0;
    float value = exponent ? ldexpf(1024 + fraction, (int)exponent - 25)
                           : ldexpf(fraction, -24);
    return bits & 32768 ? -value : value;
}

EXPORT int phitkc_prores_open(const char *path, int width, int height, int fps,
                             int rate, int profile, int alpha, int transfer,
                             ProRes **output) {
    int result = AVERROR(ENOMEM);
    ProRes *writer = calloc(1, sizeof(*writer));
    if (!writer)
        return result;
    writer->alpha = alpha;
    writer->transfer = transfer;
    if (transfer == 1) {
        writer->pq = malloc(65536 * sizeof(*writer->pq));
        if (!writer->pq)
            goto fail;
        for (unsigned i = 0; i < 65536; i++)
            writer->pq[i] = pq_encode(half_float(i));
    }
    const AVCodec *codec = avcodec_find_encoder_by_name("prores_ks");
    if (!codec) {
        result = AVERROR_ENCODER_NOT_FOUND;
        goto fail;
    }
    result = avformat_alloc_output_context2(&writer->format, NULL, "mov", path);
    if (result < 0)
        goto fail;
    result = AVERROR(ENOMEM);
    writer->codec = avcodec_alloc_context3(codec);
    writer->video = avformat_new_stream(writer->format, NULL);
    writer->audio = avformat_new_stream(writer->format, NULL);
    writer->frame = av_frame_alloc();
    writer->packet = av_packet_alloc();
    if (!writer->codec || !writer->video || !writer->audio || !writer->frame || !writer->packet)
        goto fail;

    AVCodecContext *video = writer->codec;
    video->width = width;
    video->height = height;
    video->time_base = (AVRational){1, fps};
    video->framerate = (AVRational){fps, 1};
    video->profile = profile;
    video->pix_fmt = profile < 4 ? AV_PIX_FMT_YUV422P10LE
                                 : alpha ? AV_PIX_FMT_YUVA444P10LE : AV_PIX_FMT_YUV444P10LE;
    video->color_primaries = transfer ? AVCOL_PRI_BT2020 : AVCOL_PRI_BT709;
    video->color_trc = transfer == 1 ? AVCOL_TRC_SMPTE2084
                     : transfer == 2 ? AVCOL_TRC_ARIB_STD_B67 : AVCOL_TRC_BT709;
    video->colorspace = transfer ? AVCOL_SPC_BT2020_NCL : AVCOL_SPC_BT709;
    video->color_range = AVCOL_RANGE_MPEG;
    video->thread_count = 4;
    if (profile >= 0 && profile <= 5) {
        video->profile = profile;
        video->bits_per_raw_sample = profile == 4 ? 10 : profile == 5 ? 12 : 10;
    } else {
        video->profile = AV_PROFILE_UNKNOWN;
        video->bits_per_raw_sample = 10;
    }
    if (writer->format->oformat->flags & AVFMT_GLOBALHEADER)
        video->flags |= AV_CODEC_FLAG_GLOBAL_HEADER;
    result = av_opt_set_int(video->priv_data, "profile", profile, 0);
    if (result < 0)
        goto fail;
    result = av_opt_set_int(video->priv_data, "alpha_bits", alpha ? 16 : 0, 0);
    if (result < 0)
        goto fail;
    result = avcodec_open2(video, codec, NULL);
    if (result < 0)
        goto fail;
    result = avcodec_parameters_from_context(writer->video->codecpar, video);
    if (result < 0)
        goto fail;
    writer->video->time_base = video->time_base;

    AVCodecParameters *audio = writer->audio->codecpar;
    audio->codec_type = AVMEDIA_TYPE_AUDIO;
    audio->codec_id = AV_CODEC_ID_PCM_S16LE;
    audio->format = AV_SAMPLE_FMT_S16;
    audio->sample_rate = rate;
    audio->bits_per_coded_sample = 16;
    audio->block_align = 4;
    audio->bit_rate = (int64_t)rate * 32;
    av_channel_layout_default(&audio->ch_layout, 2);
    writer->audio->time_base = (AVRational){1, rate};

    writer->frame->format = video->pix_fmt;
    writer->frame->width = width;
    writer->frame->height = height;
    writer->frame->color_primaries = video->color_primaries;
    writer->frame->color_trc = video->color_trc;
    writer->frame->colorspace = video->colorspace;
    writer->frame->color_range = video->color_range;
    result = av_frame_get_buffer(writer->frame, 32);
    if (result < 0)
        goto fail;
    result = avio_open(&writer->format->pb, path, AVIO_FLAG_WRITE);
    if (result < 0)
        goto fail;
    result = avformat_write_header(writer->format, NULL);
    if (result < 0)
        goto fail;
    *output = writer;
    return 0;

fail:
    phitkc_prores_close(writer);
    return result;
}

static int video_packets(ProRes *writer, AVFrame *frame) {
    int result = avcodec_send_frame(writer->codec, frame);
    if (result < 0)
        return result;
    while ((result = avcodec_receive_packet(writer->codec, writer->packet)) >= 0) {
        AVPacket *packet = writer->packet;
        packet->stream_index = writer->video->index;
        if (!packet->duration)
            packet->duration = 1;
        av_packet_rescale_ts(packet, writer->codec->time_base, writer->video->time_base);
        result = av_interleaved_write_frame(writer->format, packet);
        if (result < 0)
            return result;
    }
    return result == AVERROR(EAGAIN) || result == AVERROR_EOF ? 0 : result;
}

static float hlg_encode(float value) {
    return value <= 1.0f / 12.0f ? sqrtf(3.0f * value)
           : 0.17883277f * logf(12.0f * value - 0.28466892f) + 0.5599107f;
}

static void convert(ProRes *writer, const uint8_t *sample, float *y, float *cb,
                    float *cr, float *alpha) {
    float kr = writer->transfer ? 0.2627f : 0.2126f;
    float kb = writer->transfer ? 0.0593f : 0.0722f;
    float kg = 1.0f - kr - kb;
    float rgb[3];
    if (!writer->transfer) {
        for (int c = 0; c < 3; c++)
            rgb[c] = sample[c] / 255.0f;
        *alpha = sample[3] / 255.0f;
    } else {
        uint16_t bits[4];
        memcpy(bits, sample, sizeof(bits));
        *alpha = fminf(fmaxf(half_float(bits[3]), 0.0f), 1.0f);
        if (writer->transfer == 1) {
            for (int c = 0; c < 3; c++)
                rgb[c] = writer->pq[bits[c]];
        } else {
            for (int c = 0; c < 3; c++)
                rgb[c] = fminf(fmaxf(half_float(bits[c]) / 10.0f, 0.0f), 1.0f);
            float luminance = kr * rgb[0] + kg * rgb[1] + kb * rgb[2];
            float scale = luminance > 0.0f ? powf(luminance, -1.0f / 6.0f) : 0.0f;
            for (int c = 0; c < 3; c++)
                rgb[c] = hlg_encode(fminf(rgb[c] * scale, 1.0f));
        }
    }
    *y = kr * rgb[0] + kg * rgb[1] + kb * rgb[2];
    *cb = (rgb[2] - *y) / (2.0f * (1.0f - kb));
    *cr = (rgb[0] - *y) / (2.0f * (1.0f - kr));
}

EXPORT int phitkc_prores_frame(ProRes *writer, const uint8_t *rgba) {
    AVFrame *frame = writer->frame;
    int result = av_frame_make_writable(frame);
    if (result < 0)
        return result;
    int bytes_per_pixel = writer->transfer ? 8 : 4;
    int step = frame->format == AV_PIX_FMT_YUV422P10LE ? 2 : 1;
    for (int row = 0; row < frame->height; row++) {
        uint16_t *planes[4] = {0};
        for (int plane = 0; plane < 3 + writer->alpha; plane++)
            planes[plane] = (uint16_t *)(frame->data[plane] + row * frame->linesize[plane]);
        for (int column = 0; column < frame->width; column += step) {
            float cb_sum = 0.0f;
            float cr_sum = 0.0f;
            for (int i = 0; i < step; i++) {
                float y, cb, cr, alpha;
                convert(writer, rgba, &y, &cb, &cr, &alpha);
                rgba += bytes_per_pixel;
                planes[0][column + i] = lrintf(64.0f + 876.0f * y);
                cb_sum += cb;
                cr_sum += cr;
                if (writer->alpha)
                    planes[3][column + i] = lrintf(alpha * 1023.0f);
            }
            planes[1][column / step] = lrintf(512.0f + (896.0f / step) * cb_sum);
            planes[2][column / step] = lrintf(512.0f + (896.0f / step) * cr_sum);
        }
    }
    frame->pts = writer->video_count++;
    return video_packets(writer, frame);
}

EXPORT int phitkc_prores_audio(ProRes *writer, const uint8_t *pcm, int bytes) {
    if (!bytes)
        return 0;
    AVPacket *packet = writer->packet;
    int result = av_new_packet(packet, bytes);
    if (result < 0)
        return result;
    memcpy(packet->data, pcm, bytes);
    packet->stream_index = writer->audio->index;
    packet->pts = packet->dts = writer->audio_count;
    packet->duration = bytes / 4;
    packet->flags = AV_PKT_FLAG_KEY;
    writer->audio_count += packet->duration;
    return av_interleaved_write_frame(writer->format, packet);
}

EXPORT int phitkc_prores_finish(ProRes *writer) {
    int result = video_packets(writer, NULL);
    return result < 0 ? result : av_write_trailer(writer->format);
}
