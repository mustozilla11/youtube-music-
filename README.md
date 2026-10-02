# 🎵 YouTube Music — Rust Desktop Client

CachyOS / KDE Linux için hafif, saf Rust ile yazılmış YouTube Music masaüstü uygulaması.  
OLED siyah teması, düşük kaynak tüketimi, sade arayüz.

---

## Özellikler

| Özellik | Durum |
|---|---|
| Şarkı arama | ✅ |
| Şarkı çalma (mpv + yt-dlp) | ✅ |
| Son dinlenenler (geçmiş) | ✅ |
| Favoriler — ekle / çıkar | ✅ |
| Kapak resmi gösterme | ✅ |
| Play / Pause kontrolü | ✅ |
| İlerleme çubuğu + seek | ✅ |
| Ses kontrolü | ✅ |
| Cookie ile giriş (auth) | ✅ |
| OLED siyah tema | ✅ |

---

## Sistem Gereksinimleri

```bash
# mpv ve yt-dlp kurulumu (CachyOS / Arch tabanlı)
sudo pacman -S mpv yt-dlp
```

> Rust araç zinciri de gerekli:
> ```bash
> curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
> ```

---

## Kurulum ve Çalıştırma

```bash
# Projeyi klonla veya dizine git
cd ~/ytmusic-rs

# Geliştirme modunda çalıştır
cargo run

# Ya da optimized release derle
cargo build --release
./target/release/ytmusic
```

---

## Giriş Yapma (Cookie Auth)

Uygulama anonim modda da çalışır (arama + çalma).  
Kişisel özellikler için cookie ile giriş yapılabilir:

1. Tarayıcında **music.youtube.com**'a gir ve hesabına giriş yap.
2. **F12** → **Network** sekmesine geç.
3. Herhangi bir isteğe tıkla → **Request Headers** bölümüne bak.
4. `Cookie:` satırının tüm değerini kopyala.
5. Uygulamada sağ üstteki **🔐 Giriş Yap** butonuna tıkla, yapıştır ve kaydet.
6. Değişikliğin geçerli olması için uygulamayı yeniden başlat.

---

## Mimari

```
ytmusic-rs/
├── src/
│   ├── main.rs      # Giriş noktası (tokio runtime + eframe başlatma)
│   ├── app.rs       # egui UI (OLED tema, sekmeler, oynatıcı barı, login)
│   ├── api.rs       # YouTube Music internal API + yt-dlp stream çözümleyici
│   ├── player.rs    # mpv IPC sarmalayıcı (Unix socket üzerinden JSON)
│   ├── storage.rs   # JSON kalıcılığı (~/.local/share/ytmusic-rs/)
│   └── types.rs     # Paylaşılan tipler (Track, Tab, WorkerMsg, AppResult)
└── Cargo.toml
```

### Kullanılan Teknolojiler

| Katman | Teknoloji |
|---|---|
| GUI | `egui` / `eframe` 0.28 (immediate-mode, Wayland + X11) |
| Ses çalma | `mpv` (sistem genelinde, IPC üzerinden kontrol) |
| Stream URL | `yt-dlp` subprocess |
| API | YouTube Music YouTubei v1 (informal, `reqwest`) |
| Async | `tokio` multi-thread runtime |
| Veri | `serde_json` + `~/.local/share/ytmusic-rs/` |

---

## Veri Dosyaları

```
~/.local/share/ytmusic-rs/
├── config.json      # Cookie ayarı
├── history.json     # Son 50 dinlenen şarkı
└── favorites.json   # Favori şarkılar
```

---

## Gelecek Özellikler (Eklemek İstersen)

- [ ] Kuyruk / çalma listesi
- [ ] Sonraki / önceki şarkı butonları
- [ ] Keyboard kısayolları (Space, → / ←)
- [ ] Sistem tepsisi (tray icon)
- [ ] Bildirim entegrasyonu (MPRIS / D-Bus)
- [ ] Öneri algoritması (YouTube Mix)
- [ ] Offline mod / indirme

---

## Lisans

MIT
