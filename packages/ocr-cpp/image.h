#pragma once
#include <cstdint>
#include <vector>
#include <cmath>
#include <algorithm>
#include <stdexcept>
#include <string>

#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

#ifdef _WIN32
#include <windows.h>
#endif

struct Image {
    int w, h, c;
    std::vector<uint8_t> data;
};

static Image loadImage(const char* path) {
    Image img;
    int n = 0;
    unsigned char* pixels = nullptr;

#ifdef _WIN32
    int wlen = MultiByteToWideChar(CP_ACP, 0, path, -1, nullptr, 0);
    if (wlen <= 0) {
        throw std::runtime_error(std::string("Failed to convert path to wide (err=") + std::to_string(GetLastError()) + "): " + path);
    }
    std::wstring wpath(wlen, L'\0');
    MultiByteToWideChar(CP_ACP, 0, path, -1, &wpath[0], wlen);
    FILE* f = _wfopen(wpath.c_str(), L"rb");
    if (!f) {
        DWORD err = GetLastError();
        // Fallback: try ANSI fopen
        f = fopen(path, "rb");
        if (!f) {
            throw std::runtime_error(std::string("Failed to open file (err=") + std::to_string(err) + "): " + path);
        }
    }
    pixels = stbi_load_from_file(f, &img.w, &img.h, &n, 3);
    fclose(f);
#else
    pixels = stbi_load(path, &img.w, &img.h, &n, 3);
#endif

    if (!pixels) throw std::runtime_error("Failed to load image");
    img.c = 3;
    img.data.assign(pixels, pixels + img.w * img.h * 3);
    stbi_image_free(pixels);
    return img;
}

static float sampleBilinear(const Image& img, float x, float y, int c) {
    if (x < 0 || x >= img.w || y < 0 || y >= img.h) return 0;
    int x0 = std::max(0, std::min((int)x, img.w - 2));
    int y0 = std::max(0, std::min((int)y, img.h - 2));
    int x1 = std::min(x0 + 1, img.w - 1);
    int y1 = std::min(y0 + 1, img.h - 1);
    float fx = x - x0, fy = y - y0;
    auto px = [&](int px, int py) -> float {
        return img.data[(py * img.w + px) * img.c + c];
    };
    return (1-fy)*(1-fx)*px(x0,y0) + (1-fy)*fx*px(x1,y0)
         + fy*(1-fx)*px(x0,y1) + fy*fx*px(x1,y1);
}

static void resizeBilinear(const uint8_t* src, int sw, int sh, uint8_t* dst, int dw, int dh) {
    for (int dy = 0; dy < dh; ++dy) {
        float sy = (dy + 0.5f) * sh / dh - 0.5f;
        sy = std::max(0.0f, std::min<float>(sh - 1, sy));
        int sy0 = (int)sy, sy1 = std::min(sy0 + 1, sh - 1);
        float fy = sy - sy0;
        for (int dx = 0; dx < dw; ++dx) {
            float sx = (dx + 0.5f) * sw / dw - 0.5f;
            sx = std::max(0.0f, std::min<float>(sw - 1, sx));
            int sx0 = (int)sx, sx1 = std::min(sx0 + 1, sw - 1);
            float fx = sx - sx0;
            for (int c = 0; c < 3; ++c) {
                float v = (1-fy)*(1-fx)*src[(sy0*sw+sx0)*3+c]
                        + (1-fy)*fx   *src[(sy0*sw+sx1)*3+c]
                        + fy   *(1-fx)*src[(sy1*sw+sx0)*3+c]
                        + fy   *fx   *src[(sy1*sw+sx1)*3+c];
                dst[(dy*dw+dx)*3+c] = (uint8_t)std::round(v);
            }
        }
    }
}
