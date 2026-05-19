# تقرير مشروع CarbonQueue

## ما هو التطبيق؟

CarbonQueue هو تطبيق Windows مستقل لإدارة طابور العيادة. يضيف المرضى، يولد أرقام انتظار، يستدعي الرقم التالي، ويسجل حالة الخدمة.

## كيف يعمل؟

- يعمل كتطبيق سطح مكتب عبر Tauri.
- يستخدم SQLite محلياً لحفظ الطابور.
- عند حفظ بيانات Unifonic، يحاول إرسال SMS عند استدعاء الرقم التالي.
- الترخيص مرتبط ببصمة جهاز Windows.
- مفتاح التطوير للتجربة: `CF-DEMO-ALL`.
- يدعم تحديثات موقعة من المطور.

## لغة البرمجة والتقنيات

- Frontend: TypeScript + React
- Desktop Backend: Rust + Tauri
- Database: SQLite
- SMS Integration: Unifonic REST API
- Installer: NSIS

## الملفات المهمة

- السورس: `src/`
- كود Rust: `src-tauri/src/main.rs`
- إعداد Tauri: `src-tauri/tauri.conf.json`
- المثبت الجاهز: `release/CarbonQueue_0.1.0_x64-setup.exe`
- توقيع التحديث: `release/CarbonQueue_0.1.0_x64-setup.exe.sig`

## نتيجة الفحص

- `npm run build:web`: ناجح
- `npm run lint`: ناجح
- `cargo check`: ناجح
- بناء مثبت Windows: ناجح

## نسبة نجاح التطبيق بعد الفحص

نسبة الجاهزية: 88%

السبب: الطابور المحلي والمثبت يعملان. إرسال SMS يحتاج مفاتيح Unifonic حقيقية وتجربة على حساب فعلي قبل الإنتاج.
