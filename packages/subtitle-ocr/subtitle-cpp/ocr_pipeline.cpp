#define _CRT_SECURE_NO_WARNINGS 1

#include <iostream>
#include <fstream>
#include <string>
#include <vector>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <chrono>
#include <sstream>
#include <iomanip>

#ifndef M_PI_2
#define M_PI_2 1.57079632679489661923
#endif

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOGDI
#define NOMINMAX
#include <windows.h>
static std::wstring toWide(const std::string& s) {
    int len = MultiByteToWideChar(CP_ACP, 0, s.c_str(), -1, nullptr, 0);
    std::wstring wstr(len, L'\0');
    MultiByteToWideChar(CP_ACP, 0, s.c_str(), -1, &wstr[0], len);
    return wstr;
}
#define ORT_PATH(s) toWide(s).c_str()
#else
#define ORT_PATH(s) (s).c_str()
#endif

#include "onnxruntime_cxx_api.h"
#include "image.h"
#include "geometry.h"

// --- Constants ---
constexpr int DET_LIMIT_SIDE = 736;
constexpr int CLS_H = 48;
constexpr int CLS_W = 192;
constexpr int REC_H = 48;
constexpr int REC_MAX_W = 320;

constexpr float DET_THRESH = 0.3f;

constexpr float UNCLIP_RATIO = 1.6f;
constexpr int MAX_CANDIDATES = 1000;

struct OCRResult {
    std::string text;
    struct Segment {
        std::string text;
        float confidence;
        std::vector<std::vector<int>> box; // 4x2
    };
    std::vector<Segment> segments;
    double detMs, postMs, recMs, totalMs;
};

// --- Load char list from JSON ---
// ppocr_keys.json: 6624 个元素 [ "", "'", "疗", ... ] ，第一个是占位
// 解码用字符表：index 0 = blank (CTC 跳过), 1..6623 = 6623 个字符, 6624 = ' ' (space)
// 共 6625 个 token，与 rec 模型输出维度 (N, T, 6625) 一致
static std::vector<std::string> loadCharList(const std::string& path) {
    std::ifstream f(path);
    if (!f) return {};
    std::string json((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    std::vector<std::string> chars;
    size_t pos = 0;
    while (pos < json.size() && json[pos] != '[') pos++;
    if (pos >= json.size()) return chars;
    pos++; // skip '['
    while (pos < json.size() && json[pos] != ']') {
        while (pos < json.size() && (json[pos] == ' ' || json[pos] == '\n' || json[pos] == '\r' || json[pos] == '\t' || json[pos] == ','))
            pos++;
        if (pos >= json.size() || json[pos] == ']') break;
        if (json[pos] == '"') {
            pos++; // skip opening quote
            std::string ch;
            while (pos < json.size() && json[pos] != '"') {
                if (json[pos] == '\\') {
                    pos++;
                    if (pos < json.size()) {
                        if (json[pos] == 'u') {
                            ch += '?';
                            for (int k = 0; k < 5 && pos < json.size(); k++) pos++;
                        } else {
                            ch += json[pos++];
                        }
                    }
                } else {
                    ch += json[pos++];
                }
            }
            if (pos < json.size()) pos++; // skip closing quote
            chars.push_back(ch);
        } else break;
    }
    // chars 现在是原始 6624 元素，首个是空串
    // 转换为 [blank, chars[1..end], space]
    if (chars.empty()) return {};
    std::vector<std::string> table;
    table.reserve(chars.size() + 1); // 6624 + 1 = 6625
    table.push_back(""); // index 0: blank (CTC 跳过)
    for (size_t i = 1; i < chars.size(); ++i) table.push_back(chars[i]);
    table.push_back(" "); // last index: space
    return table;
}

// --- CTC decode ---
struct CTCResult { std::string text; float confidence; };
static CTCResult ctcDecode(const float* logits, int timesteps, int numClasses, const std::vector<std::string>& charList) {
    std::string chars;
    std::vector<float> confs;
    int prev = -1; // -1 = blank
    for (int t = 0; t < timesteps; ++t) {
        const float* row = logits + t * numClasses;
        int maxIdx = 0;
        float maxVal = row[0];
        for (int c = 1; c < numClasses; ++c) {
            if (row[c] > maxVal) { maxVal = row[c]; maxIdx = c; }
        }
        if (maxIdx == 0) { prev = -1; continue; } // blank
        if (maxIdx != prev) {
            if (maxIdx < (int)charList.size()) {
                chars += charList[maxIdx];
                confs.push_back(maxVal);
            }
        }
        prev = maxIdx;
    }
    float avgConf = 0;
    for (float c : confs) avgConf += c;
    if (!confs.empty()) avgConf /= confs.size();
    return {chars, avgConf};
}

// --- Preprocess for detection ---
struct DetPreproc {
    std::vector<float> tensor; // NCHW
    int origH, origW, resizedH, resizedW;
    int yOffset; // for bottom_only: pixels cropped from top before det
};

static DetPreproc preprocessDet(const uint8_t* rgb, int H, int W, bool bottomOnly) {
    DetPreproc out;
    out.yOffset = 0;
    int roiH = H;
    const uint8_t* roiPtr = rgb;

    if (bottomOnly) {
        // 裁剪到底部 40%，跟 Python subtitle-py.py 一致
        out.yOffset = (int)(H * 0.6f);
        roiH = H - out.yOffset;
        roiPtr = rgb + out.yOffset * W * 3;
    }
    out.origH = roiH; out.origW = W;

    int newW, newH;
    if (roiH <= W) {
        newH = DET_LIMIT_SIDE;
        newW = (int)std::round((float)W * DET_LIMIT_SIDE / roiH);
    } else {
        newW = DET_LIMIT_SIDE;
        newH = (int)std::round((float)roiH * DET_LIMIT_SIDE / W);
    }
    newW = ((newW + 31) / 32) * 32;
    newH = ((newH + 31) / 32) * 32;
    out.resizedW = newW; out.resizedH = newH;

    std::vector<uint8_t> resized(newW * newH * 3);
    resizeBilinear(roiPtr, W, roiH, resized.data(), newW, newH);

    out.tensor.resize(3 * newH * newW);
    const float mean[3] = {0.485f, 0.456f, 0.406f};
    const float std[3]  = {0.229f, 0.224f, 0.225f};
    for (int y = 0; y < newH; ++y) {
        for (int x = 0; x < newW; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * newW + x) * 3 + c] / 255.0f;
                out.tensor[c * newH * newW + y * newW + x] = (pixel - mean[c]) / std[c];
            }
        }
    }
    return out;
}

// --- Preprocess for classification ---
static std::vector<float> preprocessCls(const uint8_t* rgb, int H, int W) {
    std::vector<uint8_t> resized(CLS_W * CLS_H * 3);
    resizeBilinear(rgb, W, H, resized.data(), CLS_W, CLS_H);

    std::vector<float> tensor(3 * CLS_H * CLS_W);
    for (int y = 0; y < CLS_H; ++y) {
        for (int x = 0; x < CLS_W; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * CLS_W + x) * 3 + c] / 255.0f;
                tensor[c * CLS_H * CLS_W + y * CLS_W + x] = (pixel - 0.5f) / 0.5f;
            }
        }
    }
    return tensor;
}

// --- Preprocess for recognition ---
struct RecPreproc {
    std::vector<float> tensor;
    int width;
};

static RecPreproc preprocessRec(const uint8_t* rgb, int H, int W) {
    float whRatio = (float)W / (float)H;
    int imgW = std::min(REC_MAX_W, std::max(32, (int)std::round(REC_H * whRatio)));

    std::vector<uint8_t> resized(imgW * REC_H * 3);
    resizeBilinear(rgb, W, H, resized.data(), imgW, REC_H);

    RecPreproc out;
    out.width = imgW;
    out.tensor.resize(3 * REC_H * imgW);
    for (int y = 0; y < REC_H; ++y) {
        for (int x = 0; x < imgW; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * imgW + x) * 3 + c] / 255.0f;
                out.tensor[c * REC_H * imgW + y * imgW + x] = (pixel - 0.5f) / 0.5f;
            }
        }
    }
    return out;
}

// --- DB post-processing (complete, no Python dependency) ---
static std::vector<std::pair<Polygon,float>> dbPostprocess(
    const float* heatmap, int H, int W, int origH, int origW,
    float thresh, float boxThresh, float unclipRatio, int maxCandidates)
{
    float hmax = heatmap[0];
    for (int i = 1; i < H * W; ++i) hmax = std::max(hmax, heatmap[i]);

    std::vector<float> prob(H * W);
    if (hmax > 1.0f) {
        for (int i = 0; i < H * W; ++i)
            prob[i] = 1.0f / (1.0f + std::exp(-heatmap[i]));
    } else {
        std::copy(heatmap, heatmap + H * W, prob.begin());
    }

    std::vector<uint8_t> bitmap(H * W);
    for (int i = 0; i < H * W; ++i)
        bitmap[i] = prob[i] > thresh ? 1 : 0;

    // 2x2 dilation before connectedComponents — matches Python use_dilation:true
    std::vector<uint8_t> dilated(H * W, 0);
    for (int y = 0; y < H; ++y) {
        for (int x = 0; x < W; ++x) {
            if (bitmap[y * W + x]) {
                dilated[y * W + x] = 1;
                if (x + 1 < W) dilated[y * W + x + 1] = 1;
                if (y + 1 < H) dilated[(y + 1) * W + x] = 1;
                if (x + 1 < W && y + 1 < H) dilated[(y + 1) * W + x + 1] = 1;
            }
        }
    }
    auto components = connectedComponents(dilated.data(), H, W);
    std::vector<std::pair<Polygon,float>> results;

    int nComp = std::min((int)components.size(), maxCandidates);
    for (int i = 0; i < nComp; ++i) {
        auto& comp = components[i];
        if (comp.empty()) continue;

        Polygon contour = traceContour(comp, H, W);
        if (contour.size() < 4) continue;

        auto rect = minAreaRect(contour);
        Polygon boxPts = boxPoints(rect);
        float sside = std::min(rect.width, rect.height);
        if (sside < 3) continue;

        Polygon boxPtsForScore = boxPts;
        float score = boxScore(prob.data(), H, W, boxPtsForScore);
        if (score < boxThresh) continue;

        // Expand rotated rect uniformly (pyclipper-equivalent for rectangles)
        float dist = rect.width * rect.height * unclipRatio / (2.0f * (rect.width + rect.height));
        dist = std::max(3.0f, dist);
        rect.width += 2.0f * dist;
        rect.height += 2.0f * dist;
        Polygon boxFinal = boxPoints(rect);
        {
            float sside2 = std::min(rect.width, rect.height);
            if (sside2 < 5) continue;
        }

        for (auto& p : boxFinal) {
            p.x = std::round(p.x / W * origW);
            p.y = std::round(p.y / H * origH);
            p.x = std::max(0.0f, std::min((float)origW - 1, p.x));
            p.y = std::max(0.0f, std::min((float)origH - 1, p.y));
        }

        results.push_back({boxFinal, score});
    }

    return results;
}

// --- Warp rotated crop using RotatedRect (affine rotation + bilinear sampling) ---
static Image warpRotatedCrop(const Image& img, const RotatedRect& rect) {
    int dstW = std::max(1, (int)std::round(rect.width));
    int dstH = std::max(1, (int)std::round(rect.height));
    if (dstW > img.w * 2) dstW = std::min(dstW, img.w);
    if (dstH > img.h * 2) dstH = std::min(dstH, img.h);

    Image out;
    out.w = dstW; out.h = dstH; out.c = img.c;
    out.data.resize(dstW * dstH * img.c, 0);

    float cosA = std::cos(rect.angle), sinA = std::sin(rect.angle);
    float cx = rect.center.x, cy = rect.center.y;

    for (int dy = 0; dy < dstH; ++dy) {
        float perp = dy - dstH / 2.0f;
        for (int dx = 0; dx < dstW; ++dx) {
            float proj = dx - dstW / 2.0f;
            float sx = proj * cosA - perp * sinA + cx;
            float sy = proj * sinA + perp * cosA + cy;
            for (int c = 0; c < img.c; ++c)
                out.data[(dy * dstW + dx) * img.c + c] = (uint8_t)std::round(sampleBilinear(img, sx, sy, c));
        }
    }
    return out;
}

// --- Rotate image 180° ---
static Image rotate180(const Image& img) {
    Image out;
    out.w = img.w; out.h = img.h; out.c = img.c;
    out.data.resize(img.data.size());
    for (int y = 0; y < img.h; ++y) {
        for (int x = 0; x < img.w; ++x) {
            int srcIdx = (y * img.w + x) * 3;
            int dstIdx = ((img.h - 1 - y) * img.w + (img.w - 1 - x)) * 3;
            out.data[dstIdx] = img.data[srcIdx];
            out.data[dstIdx+1] = img.data[srcIdx+1];
            out.data[dstIdx+2] = img.data[srcIdx+2];
        }
    }
    return out;
}

// --- Order points clockwise (top-left, top-right, bottom-right, bottom-left) ---
static Polygon orderPointsClockwise(const Polygon& pts) {
    std::vector<int> idx(pts.size());
    for (size_t i = 0; i < pts.size(); ++i) idx[i] = (int)i;
    std::sort(idx.begin(), idx.end(), [&](int a, int b) { return pts[a].x < pts[b].x; });

    std::vector<int> leftIdx = {idx[0], idx[1]};
    std::sort(leftIdx.begin(), leftIdx.end(), [&](int a, int b) { return pts[a].y < pts[b].y; });
    int tl = leftIdx[0], bl = leftIdx[1];

    std::vector<int> rightIdx = {idx[2], idx[3]};
    std::sort(rightIdx.begin(), rightIdx.end(), [&](int a, int b) { return pts[a].y < pts[b].y; });
    int tr = rightIdx[0], br = rightIdx[1];

    Polygon out;
    out.push_back(pts[tl]);
    out.push_back(pts[tr]);
    out.push_back(pts[br]);
    out.push_back(pts[bl]);
    return out;
}

// --- Perspective warp crop (Python rapidocr: get_rotate_crop_image) ---
static Image warpPerspectiveCrop(const Image& img, const Polygon& pts) {
    float x0 = pts[0].x, y0 = pts[0].y;
    float x1 = pts[1].x, y1 = pts[1].y;
    float x2 = pts[2].x, y2 = pts[2].y;
    float x3 = pts[3].x, y3 = pts[3].y;

    float w1 = std::sqrt((x1 - x0) * (x1 - x0) + (y1 - y0) * (y1 - y0));
    float w2 = std::sqrt((x2 - x3) * (x2 - x3) + (y2 - y3) * (y2 - y3));
    float h1 = std::sqrt((x3 - x0) * (x3 - x0) + (y3 - y0) * (y3 - y0));
    float h2 = std::sqrt((x2 - x1) * (x2 - x1) + (y2 - y1) * (y2 - y1));

    int dstW = std::max(4, (int)std::round(std::max(w1, w2)));
    int dstH = std::max(4, (int)std::round(std::max(h1, h2)));

    bool rotate90 = (float)dstH / (float)dstW >= 1.5f;
    int outW = rotate90 ? dstH : dstW;
    int outH = rotate90 ? dstW : dstH;

    // Solve perspective transform: src pts -> dst rect corners
    float u0 = 0, v0 = 0;
    float u1 = (float)(dstW - 1), v1 = 0;
    float u2 = (float)(dstW - 1), v2 = (float)(dstH - 1);
    float u3 = 0, v3 = (float)(dstH - 1);

    double mat[8][9] = {
        {u0, v0, 1,  0,  0, 0, -u0 * x0, -v0 * x0, x0},
        {u1, v1, 1,  0,  0, 0, -u1 * x1, -v1 * x1, x1},
        {u2, v2, 1,  0,  0, 0, -u2 * x2, -v2 * x2, x2},
        {u3, v3, 1,  0,  0, 0, -u3 * x3, -v3 * x3, x3},
        { 0,  0, 0, u0, v0, 1, -u0 * y0, -v0 * y0, y0},
        { 0,  0, 0, u1, v1, 1, -u1 * y1, -v1 * y1, y1},
        { 0,  0, 0, u2, v2, 1, -u2 * y2, -v2 * y2, y2},
        { 0,  0, 0, u3, v3, 1, -u3 * y3, -v3 * y3, y3},
    };

    // Gaussian elimination with partial pivoting
    for (int i = 0; i < 8; ++i) {
        int maxRow = i;
        double maxVal = std::abs(mat[i][i]);
        for (int k = i + 1; k < 8; ++k) {
            double v = std::abs(mat[k][i]);
            if (v > maxVal) { maxVal = v; maxRow = k; }
        }
        if (maxRow != i) {
            for (int j = 0; j < 9; ++j) std::swap(mat[i][j], mat[maxRow][j]);
        }
        double pivot = mat[i][i];
        if (std::abs(pivot) < 1e-10) {
            // fallback: axis-aligned crop
            int axMin = (int)std::floor(std::min({x0, x1, x2, x3}));
            int axMax = (int)std::ceil(std::max({x0, x1, x2, x3}));
            int ayMin = (int)std::floor(std::min({y0, y1, y2, y3}));
            int ayMax = (int)std::ceil(std::max({y0, y1, y2, y3}));
            axMin = std::max(0, axMin); axMax = std::min(img.w, axMax);
            ayMin = std::max(0, ayMin); ayMax = std::min(img.h, ayMax);
            Image fb;
            fb.w = std::max(1, axMax - axMin);
            fb.h = std::max(1, ayMax - ayMin);
            fb.c = 3;
            fb.data.resize(fb.w * fb.h * 3);
            for (int y = 0; y < fb.h; ++y) {
                for (int x = 0; x < fb.w; ++x) {
                    int si = ((y + ayMin) * img.w + (x + axMin)) * 3;
                    int di = (y * fb.w + x) * 3;
                    fb.data[di] = img.data[si];
                    fb.data[di+1] = img.data[si+1];
                    fb.data[di+2] = img.data[si+2];
                }
            }
            return fb;
        }
        for (int j = i; j < 9; ++j) mat[i][j] /= pivot;
        for (int k = 0; k < 8; ++k) {
            if (k != i && std::abs(mat[k][i]) > 1e-10) {
                double factor = mat[k][i];
                for (int j = i; j < 9; ++j) mat[k][j] -= factor * mat[i][j];
            }
        }
    }

    double a = mat[0][8], b = mat[1][8], c = mat[2][8];
    double d = mat[3][8], e = mat[4][8], f = mat[5][8];
    double g = mat[6][8], h = mat[7][8];

    Image out;
    out.w = outW; out.h = outH; out.c = 3;
    out.data.resize(outW * outH * 3, 0);

    for (int dy = 0; dy < outH; ++dy) {
        for (int dx = 0; dx < outW; ++dx) {
            float u, v;
            if (rotate90) {
                u = (float)(dstW - 1 - dy);
                v = (float)dx;
            } else {
                u = (float)dx;
                v = (float)dy;
            }
            double denom = g * u + h * v + 1.0;
            float sx = (float)((a * u + b * v + c) / denom);
            float sy = (float)((d * u + e * v + f) / denom);
            int di = (dy * outW + dx) * 3;
            for (int ch = 0; ch < 3; ++ch) {
                out.data[di + ch] = (uint8_t)std::round(sampleBilinear(img, sx, sy, ch));
            }
        }
    }
    return out;
}

// --- Run full OCR pipeline ---
static OCRResult runOcr(
    const std::string& imagePath,
    const std::vector<std::string>& charList,
    Ort::Session& detSession,
    Ort::Session& clsSession,
    Ort::Session& recSession,
    Ort::MemoryInfo& memInfo,
    float textScore = 0.5f,
    bool subtitleOnly = false)
{
    using clock = std::chrono::high_resolution_clock;
    auto tStart = clock::now();

    OCRResult result;
    result.detMs = result.postMs = result.recMs = 0;

    // Load image
    std::cerr << "[OCR] loading image..." << std::endl;
    auto img = loadImage(imagePath.c_str());
    std::cerr << "[OCR] image " << img.w << "x" << img.h << std::endl;

    // --- Detection ---
    auto t0 = clock::now();
    std::cerr << "[OCR] preprocess det..." << std::endl;
    auto detPrep = preprocessDet(img.data.data(), img.h, img.w, subtitleOnly);

    // Det inference
    std::cerr << "[OCR] det inference " << detPrep.resizedH << "x" << detPrep.resizedW << "..." << std::endl;
    std::vector<int64_t> detShape = {1, 3, detPrep.resizedH, detPrep.resizedW};
    Ort::Value detInput = Ort::Value::CreateTensor<float>(
        memInfo, detPrep.tensor.data(), detPrep.tensor.size(), detShape.data(), detShape.size());

    auto detInNames = detSession.GetInputNames();
    auto detOutNames = detSession.GetOutputNames();
    std::vector<const char*> detInputNames = {detInNames[0].c_str()};
    std::vector<const char*> detOutputNames = {detOutNames[0].c_str()};

    auto detOut = detSession.Run(Ort::RunOptions{nullptr}, detInputNames.data(), &detInput, 1,
                                 detOutputNames.data(), 1);
    float* heatmapData = detOut[0].GetTensorMutableData<float>();
    auto detShapeOut = detOut[0].GetTensorTypeAndShapeInfo().GetShape();
    int outH = (int)detShapeOut[2], outW = (int)detShapeOut[3];

    auto t1 = clock::now();
    result.detMs = std::chrono::duration<double, std::milli>(t1 - t0).count();

    // --- DB post-process ---
    // 关键：坐标先映射到 ROI 空间 (detPrep.origH x detPrep.origW)，
    // 然后在下面的 for 循环中 + yOffset 映射回原图（跟 Node.js 一致）
    std::cerr << "[OCR] dbPostprocess " << outH << "x" << outW << " (orig " << detPrep.origH << "x" << detPrep.origW << ")..." << std::endl;
    auto boxesPts = dbPostprocess(heatmapData, outH, outW, detPrep.origH, detPrep.origW,
                                  DET_THRESH, textScore, UNCLIP_RATIO, MAX_CANDIDATES);

    auto t2 = clock::now();
    result.postMs = std::chrono::duration<double, std::milli>(t2 - t1).count();

    // --- Recognition for each box ---
    int boxIdx = 0;
    for (auto& [boxPts, score] : boxesPts) {
        (void)score;
        if (boxPts.size() < 4) continue;

        // box 坐标现在是 ROI 内部的坐标。如果开启了 bottom_only，先加回 yOffset 以便在原图做裁剪
        if (detPrep.yOffset > 0) {
            for (auto& p : boxPts) p.y += (float)detPrep.yOffset;
        }

        // Y-position filter for subtitle only
        if (subtitleOnly) {
            float yMinCk = boxPts[0].y, yMaxCk = boxPts[0].y;
            for (auto& p : boxPts) {
                yMinCk = std::min(yMinCk, p.y);
                yMaxCk = std::max(yMaxCk, p.y);
            }
            float yCenter = (yMinCk + yMaxCk) / 2;
            if (yCenter < (float)(img.h * 0.6f) || yCenter > (float)img.h) continue;
        }

        // 对齐 Python rapidocr: orderPointsClockwise 排序 + warpPerspective 裁剪
        Polygon orderedPts = orderPointsClockwise(boxPts);
        for (auto& p : orderedPts) {
            p.x = std::max(0.0f, std::min((float)img.w - 1.0f, p.x));
            p.y = std::max(0.0f, std::min((float)img.h - 1.0f, p.y));
        }
        Image crop = warpPerspectiveCrop(img, orderedPts);
        if (crop.w < 4 || crop.h < 4) continue;
        boxPts = orderedPts;

        // Cls inference
        auto clsTensor = preprocessCls(crop.data.data(), crop.h, crop.w);
        std::vector<int64_t> clsShape = {1, 3, CLS_H, CLS_W};
        Ort::Value clsInputVal = Ort::Value::CreateTensor<float>(
            memInfo, clsTensor.data(), clsTensor.size(), clsShape.data(), clsShape.size());

        auto clsInNames = clsSession.GetInputNames();
        auto clsOutNames = clsSession.GetOutputNames();
        std::vector<const char*> clsInputNames = {clsInNames[0].c_str()};
        std::vector<const char*> clsOutputNames = {clsOutNames[0].c_str()};

        auto clsOut = clsSession.Run(Ort::RunOptions{nullptr}, clsInputNames.data(), &clsInputVal, 1,
                                     clsOutputNames.data(), 1);
        float* clsData = clsOut[0].GetTensorMutableData<float>();
        bool rotate = clsData[0] < clsData[1];

        // Rotate if needed
        Image recCrop = rotate ? rotate180(crop) : crop;

        // Rec inference
        auto recPrep = preprocessRec(recCrop.data.data(), recCrop.h, recCrop.w);

        auto t3 = clock::now();
        std::vector<int64_t> recShape = {1, 3, REC_H, recPrep.width};
        Ort::Value recInputVal = Ort::Value::CreateTensor<float>(
            memInfo, recPrep.tensor.data(), recPrep.tensor.size(), recShape.data(), recShape.size());

        auto recInNames = recSession.GetInputNames();
        auto recOutNames = recSession.GetOutputNames();
        std::vector<const char*> recInputNames = {recInNames[0].c_str()};
        std::vector<const char*> recOutputNames = {recOutNames[0].c_str()};

        auto recOut = recSession.Run(Ort::RunOptions{nullptr}, recInputNames.data(), &recInputVal, 1,
                                     recOutputNames.data(), 1);

        auto t4 = clock::now();
        result.recMs += std::chrono::duration<double, std::milli>(t4 - t3).count();

        float* recData = recOut[0].GetTensorMutableData<float>();
        auto recShapeOut = recOut[0].GetTensorTypeAndShapeInfo().GetShape();
        int timesteps = (int)recShapeOut[1];
        int numClasses = (int)recShapeOut[2];

        auto [text, conf] = ctcDecode(recData, timesteps, numClasses, charList);
        if (text.empty()) continue;
        if (conf < textScore) continue;

        // Convert box to integer coordinates for output
        std::vector<std::vector<int>> boxOut;
        for (auto& p : boxPts) {
            boxOut.push_back({(int)std::round(p.x), (int)std::round(p.y)});
        }

        result.segments.push_back({text, conf, std::move(boxOut)});
        boxIdx++;
    }

    // Sort top-to-bottom, left-to-right
    std::sort(result.segments.begin(), result.segments.end(),
        [](auto& a, auto& b) {
            auto boxCenterY = [](auto& s) {
                float sum = 0;
                for (auto& p : s.box) sum += p[1];
                return sum / s.box.size();
            };
            auto boxCenterX = [](auto& s) {
                float sum = 0;
                for (auto& p : s.box) sum += p[0];
                return sum / s.box.size();
            };
            float ya = boxCenterY(a), yb = boxCenterY(b);
            if (std::abs(ya - yb) > 20) return ya < yb;
            return boxCenterX(a) < boxCenterX(b);
        });

    // Build full text
    for (auto& seg : result.segments)
        result.text += seg.text;

    auto tEnd = clock::now();
    result.totalMs = std::chrono::duration<double, std::milli>(tEnd - tStart).count();

    return result;
}

// --- JSON output ---
static std::string toJson(const OCRResult& r) {
    std::ostringstream ss;
    ss << std::fixed << std::setprecision(2);
    ss << "{\n";
    ss << "  \"text\": " << std::quoted(r.text) << ",\n";
    ss << "  \"segments\": [\n";
    for (size_t i = 0; i < r.segments.size(); ++i) {
        auto& seg = r.segments[i];
        ss << "    {\"text\": " << std::quoted(seg.text)
           << ", \"confidence\": " << seg.confidence
           << ", \"box\": [";
        for (size_t j = 0; j < seg.box.size(); ++j) {
            ss << "[" << seg.box[j][0] << "," << seg.box[j][1] << "]";
            if (j + 1 < seg.box.size()) ss << ",";
        }
        ss << "]}";
        if (i + 1 < r.segments.size()) ss << ",";
        ss << "\n";
    }
    ss << "  ],\n";
    ss << "  \"detInferenceMs\": " << r.detMs << ",\n";
    ss << "  \"postprocessMs\": " << r.postMs << ",\n";
    ss << "  \"recInferenceMs\": " << r.recMs << ",\n";
    ss << "  \"totalMs\": " << r.totalMs << "\n";
    ss << "}\n";
    return ss.str();
}

static int runMain(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: " << argv[0] << " <image_path> [text_score] [--subtitle-only] [--device cpu|cuda|dml|coreml|rocm]" << std::endl;
        return 1;
    }

    std::string imagePath = argv[1];
    float textScore = 0.5f;
    bool subtitleOnly = false;
    std::string device = "cpu";
    for (int i = 2; i < argc; ++i) {
        if (strcmp(argv[i], "--subtitle-only") == 0) subtitleOnly = true;
        else if (strcmp(argv[i], "--device") == 0 && i + 1 < argc) { device = argv[++i]; }
        else if (argv[i][0] != '-') textScore = std::stof(argv[i]);
    }

    try {
        // Find model directory (set by TypeScript wrapper via OCR_MODELS_DIR)
        std::string modelDir;
        const char* envModels = std::getenv("OCR_MODELS_DIR");
        if (!envModels) {
            std::cerr << "OCR_MODELS_DIR not set. The TypeScript wrapper should set this." << std::endl;
            return 1;
        }
        modelDir = envModels;

        // Find char list (set by TypeScript wrapper via OCR_KEYS_PATH)
        std::string keysPath;
        const char* envKeys = std::getenv("OCR_KEYS_PATH");
        if (!envKeys) {
            std::cerr << "OCR_KEYS_PATH not set. The TypeScript wrapper should set this." << std::endl;
            return 1;
        }
        keysPath = envKeys;

        auto charList = loadCharList(keysPath);
        if (charList.empty()) {
            std::cerr << "Failed to load char list from " << keysPath << std::endl;
            return 1;
        }

        // Initialize ORT
        Ort::Env env(ORT_LOGGING_LEVEL_WARNING, "ocr");
        Ort::SessionOptions sessionOptions;
        sessionOptions.SetIntraOpNumThreads(4);
        sessionOptions.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_ALL);

        // Increase session timeout for model loading
        sessionOptions.SetExecutionMode(ExecutionMode::ORT_SEQUENTIAL);

        // Configure execution provider based on device
        if (device == "cuda") {
            try {
                OrtCUDAProviderOptions cudaOpts{};
                sessionOptions.AppendExecutionProvider_CUDA(cudaOpts);
                std::cerr << "[OCR] Using CUDA execution provider" << std::endl;
            } catch (const std::exception& e) {
                std::cerr << "[OCR] CUDA EP unavailable (" << e.what() << "), falling back to CPU" << std::endl;
            }
        // Note: DML/CoreML/ROCm providers not available in official onnxruntime build
        // These would require building onnxruntime from source with respective options
        } else {
            std::cerr << "[OCR] Using CPU execution provider" << std::endl;
        }

        auto memInfo = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);

        // Load models
        auto detPath = modelDir + "/ch_PP-OCRv3_det_infer.onnx";
        auto clsPath = modelDir + "/ch_ppocr_mobile_v2.0_cls_infer.onnx";
        auto recPath = modelDir + "/ch_PP-OCRv3_rec_infer.onnx";

        Ort::Session detSession(env, ORT_PATH(detPath), sessionOptions);
        Ort::Session clsSession(env, ORT_PATH(clsPath), sessionOptions);
        Ort::Session recSession(env, ORT_PATH(recPath), sessionOptions);

        // Run OCR
        auto result = runOcr(imagePath, charList, detSession, clsSession, recSession, memInfo,
                             textScore, subtitleOnly);

        // Output JSON
        std::cout << toJson(result);

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}

int main(int argc, char* argv[]) {
    try {
        return runMain(argc, argv);
    } catch (const std::exception& e) {
        std::cerr << "Exception: " << e.what() << std::endl;
        return 1;
    }
}
