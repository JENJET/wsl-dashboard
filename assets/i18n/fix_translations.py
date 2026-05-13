#!/usr/bin/env python3
"""Complete missing translations for all language files - FIXED."""
import os
import re

BASE = r"D:\projects\wsl-dashboard\assets\i18n"

# Translation data: 22 elements per language
# (url_str, srcUrl, custom, customPH, browse, fetchBtn, fetchProg,
#  fetchFailed, distro, threads, arch, x64, arm64,
#  downloading, mb, shaFail, shaOk, importing, installed,
#  usingThreads, noUrl, dlFailed)

DATA = {
    "zh-TW": (
        'url = "URL 下載"', "選擇發行版來源 URL", "自定義 URL", "請輸入自定義 URL...",
        "瀏覽本地文件", "獲取發行版列表", "正在獲取...", "獲取發行版列表失敗：{0}",
        "選擇發行版", "下載線程數（1-8）", "架構選擇", "AMD64 (x86_64)", "ARM64",
        "[1/4] 正在下載 {0}...", "{0} / {1} MB（{2}%）",
        "SHA256 驗證失敗（{0}/{1}），正在重試...", "[2/4] 下載驗證成功。",
        "[3/4] 正在導入下載的發行版...", "[4/4] 發行版 '{0}' 安裝成功。",
        "使用 {0} 個線程下載", "該發行版沒有可用的下載地址", "下載失敗，已重試 {0} 次：{1}"),
    "ar": (
        'url = "تنزيل URL"', "حدد عنوان URL لمصدر التوزيع", "URL مخصص",
        "أدخل URL مخصص...", "تصفح الملف المحلي", "جلب التوزيعات", "جارٍ الجلب...",
        "فشل في جلب التوزيعات: {0}", "حدد التوزيع", "خيوط التنزيل (1-8)", "البنية",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] جارٍ تنزيل {0}...", "{0} / {1} ميغابايت ({2}%)",
        "فشل التحقق من SHA256 ({0}/{1})، إعادة المحاولة...", "[2/4] تم التحقق من التنزيل بنجاح.",
        "[3/4] جارٍ استيراد التوزيع الذي تم تنزيله...", "[4/4] تم تثبيت التوزيع '{0}' بنجاح.",
        "التنزيل باستخدام {0} من الخيوط", "لا يوجد عنوان URL للتنزيل متاح لهذا التوزيع",
        "فشل التنزيل بعد {0} من المحاولات: {1}"),
    "bn": (
        'url = "URL ডাউনলোড"', "বিতরণ উৎস URL নির্বাচন করুন", "কাস্টম URL",
        "কাস্টম URL লিখুন...", "স্থানীয় ফাইল ব্রাউজ করুন", "বিতরণ আনুন", "আনা হচ্ছে...",
        "বিতরণ আনা ব্যর্থ: {0}", "বিতরণ নির্বাচন করুন", "ডাউনলোড থ্রেড (1-8)", "স্থাপত্য",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} ডাউনলোড করা হচ্ছে...", "{0} / {1} MB ({2}%)",
        "SHA256 যাচাই ব্যর্থ ({0}/{1}), পুনরায় চেষ্টা...", "[2/4] ডাউনলোড সফলভাবে যাচাই হয়েছে।",
        "[3/4] ডাউনলোড করা বিতরণ আমদানি করা হচ্ছে...", "[4/4] বিতরণ '{0}' সফলভাবে ইনস্টল হয়েছে।",
        "{0} থ্রেড ব্যবহার করে ডাউনলোড", "এই বিতরণের জন্য কোনো ডাউনলোড URL নেই",
        "{0} বার চেষ্টার পর ডাউনলোড ব্যর্থ: {1}"),
    "cs": (
        'url = "Stažení z URL"', "Vyberte URL zdroje distribuce", "Vlastní URL",
        "Zadejte vlastní URL...", "Procházet lokální soubor", "Načíst distribuce", "Načítání...",
        "Načítání distribucí selhalo: {0}", "Vyberte distribuci", "Stahovací vlákna (1-8)", "Architektura",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Stahování {0}...", "{0} / {1} MB ({2}%)",
        "SHA256 ověření selhalo ({0}/{1}), opakování...", "[2/4] Stažení úspěšně ověřeno.",
        "[3/4] Import stažené distribuce...", "[4/4] Distribuce '{0}' úspěšně nainstalována.",
        "Stahování pomocí {0} vláken", "Pro tuto distribuci není k dispozici URL ke stažení",
        "Stahování selhalo po {0} pokusech: {1}"),
    "da": (
        'url = "URL-download"', "Vælg distributionskilde-URL", "Brugerdefineret URL",
        "Indtast brugerdefineret URL...", "Gennemse lokal fil", "Hent distributioner", "Henter...",
        "Kunne ikke hente distributioner: {0}", "Vælg distribution", "Download-tråde (1-8)", "Arkitektur",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Downloader {0}...", "{0} / {1} MB ({2}%)",
        "SHA256-bekræftelse mislykkedes ({0}/{1}), prøver igen...", "[2/4] Download bekræftet.",
        "[3/4] Importerer downloadet distribution...", "[4/4] Distribution '{0}' installeret.",
        "Download med {0} tråde", "Ingen download-URL for denne distribution",
        "Download mislykkedes efter {0} forsøg: {1}"),
    "de": (
        'url = "URL-Download"', "Wählen Sie die Quell-URL der Distribution", "Benutzerdefinierte URL",
        "Benutzerdefinierte URL eingeben...", "Lokale Datei durchsuchen", "Distributionen abrufen",
        "Abrufen...", "Fehler beim Abrufen der Distributionen: {0}", "Distribution auswählen",
        "Download-Threads (1-8)", "Architektur", "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} wird heruntergeladen...", "{0} / {1} MB ({2}%)",
        "SHA256-Überprüfung fehlgeschlagen ({0}/{1}), wiederhole...", "[2/4] Download erfolgreich überprüft.",
        "[3/4] Importiere heruntergeladene Distribution...", "[4/4] Distribution '{0}' erfolgreich installiert.",
        "Download mit {0} Threads", "Keine Download-URL für diese Distribution verfügbar",
        "Download nach {0} Versuchen fehlgeschlagen: {1}"),
    "el": (
        'url = "Λήψη από URL"', "Επιλέξτε URL πηγής διανομής", "Προσαρμοσμένο URL",
        "Εισαγάγετε προσαρμοσμένο URL...", "Περιήγηση τοπικού αρχείου", "Λήψη διανομών", "Λήψη...",
        "Αποτυχία λήψης διανομών: {0}", "Επιλογή διανομής", "Νήματα λήψης (1-8)", "Αρχιτεκτονική",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Λήψη {0}...", "{0} / {1} MB ({2}%)",
        "Αποτυχία επαλήθευσης SHA256 ({0}/{1}), επανάληψη...", "[2/4] Η λήψη επαληθεύτηκε επιτυχώς.",
        "[3/4] Εισαγωγή της ληφθείσας διανομής...", "[4/4] Η διανομή '{0}' εγκαταστάθηκε επιτυχώς.",
        "Λήψη με {0} νήματα", "Δεν υπάρχει διαθέσιμο URL λήψης για αυτήν τη διανομή",
        "Η λήψη απέτυχε μετά από {0} προσπάθειες: {1}"),
    "es": (
        'url = "Descarga por URL"', "Seleccione la URL de origen de la distribución", "URL personalizada",
        "Ingrese URL personalizada...", "Examinar archivo local", "Obtener distribuciones", "Obteniendo...",
        "Error al obtener distribuciones: {0}", "Seleccionar distribución", "Hilos de descarga (1-8)",
        "Arquitectura", "AMD64 (x86_64)", "ARM64",
        "[1/4] Descargando {0}...", "{0} / {1} MB ({2}%)",
        "Verificación SHA256 fallida ({0}/{1}), reintentando...", "[2/4] Descarga verificada con éxito.",
        "[3/4] Importando distribución descargada...", "[4/4] Distribución '{0}' instalada con éxito.",
        "Descargar usando {0} hilos", "No hay URL de descarga disponible para esta distribución",
        "Descarga fallida después de {0} intentos: {1}"),
    "fi": (
        'url = "URL-lataus"', "Valitse jakelun lähde-URL", "Mukautettu URL",
        "Syötä mukautettu URL...", "Selaa paikallista tiedostoa", "Nouda jakelut", "Noudetaan...",
        "Jakeluiden nouto epäonnistui: {0}", "Valitse jakelu", "Lataussäikeet (1-8)", "Arkkitehtuuri",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Ladataan {0}...", "{0} / {1} MB ({2}%)",
        "SHA256-vahvistus epäonnistui ({0}/{1}), yritetään uudelleen...", "[2/4] Lataus vahvistettu.",
        "[3/4] Tuodaan ladattu jakelu...", "[4/4] Jakelu '{0}' asennettu.",
        "Lataus käyttäen {0} säiettä", "Lataus-URL ei saatavilla tälle jakelulle",
        "Lataus epäonnistui {0} yrityksen jälkeen: {1}"),
    "fr": (
        'url = "Téléchargement par URL"', "Sélectionnez l'URL source de la distribution",
        "URL personnalisée", "Entrez une URL personnalisée...", "Parcourir le fichier local",
        "Récupérer les distributions", "Récupération...", "Échec de la récupération des distributions : {0}",
        "Sélectionner la distribution", "Threads de téléchargement (1-8)", "Architecture",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Téléchargement de {0}...", "{0} / {1} Mo ({2}%)",
        "Échec de la vérification SHA256 ({0}/{1}), nouvelle tentative...", "[2/4] Téléchargement vérifié avec succès.",
        "[3/4] Importation de la distribution téléchargée...", "[4/4] Distribution '{0}' installée avec succès.",
        "Téléchargement avec {0} threads", "Aucune URL de téléchargement disponible pour cette distribution",
        "Échec du téléchargement après {0} tentatives : {1}"),
    "he": (
        'url = "הורדת URL"', "בחר כתובת URL של מקור ההפצה", "כתובת URL מותאמת אישית",
        "הזן כתובת URL מותאמת אישית...", "עיין בקובץ מקומי", "הבא הפצות", "מביא...",
        "הבאת ההפצות נכשלה: {0}", "בחר הפצה", "חוטי הורדה (1-8)", "ארכיטקטורה",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] מוריד {0}...", "{0} / {1} MB ({2}%)",
        "אימות SHA256 נכשל ({0}/{1}), מנסה שוב...", "[2/4] ההורדה אומתה בהצלחה.",
        "[3/4] מייבא את ההפצה שהורדה...", "[4/4] ההפצה '{0}' הותקנה בהצלחה.",
        "הורדה באמצעות {0} חוטים", "אין כתובת URL להורדה זמינה עבור הפצה זו",
        "ההורדה נכשלה לאחר {0} ניסיונות: {1}"),
    "hi": (
        'url = "URL डाउनलोड"', "वितरण स्रोत URL चुनें", "कस्टम URL",
        "कस्टम URL दर्ज करें...", "स्थानीय फ़ाइल ब्राउज़ करें", "वितरण प्राप्त करें",
        "प्राप्त किया जा रहा है...", "वितरण प्राप्त करने में विफल: {0}", "वितरण चुनें",
        "डाउनलोड थ्रेड (1-8)", "आर्किटेक्चर", "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} डाउनलोड किया जा रहा है...", "{0} / {1} MB ({2}%)",
        "SHA256 सत्यापन विफल ({0}/{1}), पुनः प्रयास...", "[2/4] डाउनलोड सफलतापूर्वक सत्यापित हुआ।",
        "[3/4] डाउनलोड किए गए वितरण को आयात किया जा रहा है...", "[4/4] वितरण '{0}' सफलतापूर्वक स्थापित हुआ।",
        "{0} थ्रेड का उपयोग कर डाउनलोड", "इस वितरण के लिए कोई डाउनलोड URL उपलब्ध नहीं है",
        "{0} प्रयासों के बाद डाउनलोड विफल: {1}"),
    "hu": (
        'url = "URL letöltés"', "Válassza ki a disztribúció forrás URL-jét", "Egyéni URL",
        "Adja meg az egyéni URL-t...", "Helyi fájl tallózása", "Disztribúciók lekérése", "Lekérés...",
        "Disztribúciók lekérése sikertelen: {0}", "Disztribúció kiválasztása", "Letöltési szálak (1-8)",
        "Architektúra", "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} letöltése...", "{0} / {1} MB ({2}%)",
        "SHA256 ellenőrzés sikertelen ({0}/{1}), újra...", "[2/4] Letöltés sikeresen ellenőrizve.",
        "[3/4] Letöltött disztribúció importálása...", "[4/4] '{0}' disztribúció sikeresen telepítve.",
        "Letöltés {0} szállal", "Nincs letöltési URL ehhez a disztribúcióhoz",
        "Letöltés sikertelen {0} próbálkozás után: {1}"),
    "id": (
        'url = "URL Unduh"', "Pilih URL sumber distribusi", "URL Kustom",
        "Masukkan URL kustom...", "Jelajahi File Lokal", "Ambil Distribusi", "Mengambil...",
        "Gagal mengambil distribusi: {0}", "Pilih distribusi", "Utas unduhan (1-8)", "Arsitektur",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Mengunduh {0}...", "{0} / {1} MB ({2}%)",
        "Verifikasi SHA256 gagal ({0}/{1}), mencoba ulang...", "[2/4] Unduhan berhasil diverifikasi.",
        "[3/4] Mengimpor distribusi yang diunduh...", "[4/4] Distribusi '{0}' berhasil diinstal.",
        "Unduh menggunakan {0} utas", "Tidak ada URL unduhan yang tersedia untuk distribusi ini",
        "Unduhan gagal setelah {0} kali percobaan: {1}"),
    "is": (
        'url = "URL niðurhal"', "Veldu URL upprunar dreifingar", "Sérsniðið URL",
        "Sláðu inn sérsniðið URL...", "Fletta staðbundna skrá", "Sækja dreifingar", "Sæki...",
        "Mistókst að sækja dreifingar: {0}", "Veldu dreifingu", "Niðurhalsþræðir (1-8)", "Arkitektúr",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Sæki {0}...", "{0} / {1} MB ({2}%)",
        "SHA256 staðfesting mistókst ({0}/{1}), reyni aftur...", "[2/4] Niðurhal staðfest.",
        "[3/4] Fly inn sótta dreifingu...", "[4/4] Dreifing '{0}' uppsett.",
        "Niðurhal með {0} þráðum", "Ekkert niðurhals URL fyrir þessa dreifingu",
        "Niðurhal mistókst eftir {0} tilraunir: {1}"),
    "it": (
        'url = "Download da URL"', "Seleziona l'URL di origine della distribuzione",
        "URL personalizzato", "Inserisci URL personalizzato...", "Sfoglia file locale",
        "Recupera distribuzioni", "Recupero...", "Recupero distribuzioni fallito: {0}",
        "Seleziona distribuzione", "Thread di download (1-8)", "Architettura",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] Download di {0} in corso...", "{0} / {1} MB ({2}%)",
        "Verifica SHA256 fallita ({0}/{1}), riprovo...", "[2/4] Download verificato con successo.",
        "[3/4] Importazione della distribuzione scaricata...", "[4/4] Distribuzione '{0}' installata con successo.",
        "Download con {0} thread", "Nessun URL di download disponibile per questa distribuzione",
        "Download fallito dopo {0} tentativi: {1}"),
    "ja": (
        'url = "URL ダウンロード"', "ディストリビューションソース URL を選択", "カスタム URL",
        "カスタム URL を入力...", "ローカルファイルを参照", "ディストリビューションを取得", "取得中...",
        "ディストリビューションの取得に失敗: {0}", "ディストリビューションを選択",
        "ダウンロードスレッド (1-8)", "アーキテクチャ", "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} をダウンロード中...", "{0} / {1} MB ({2}%)",
        "SHA256 検証に失敗 ({0}/{1})、再試行中...", "[2/4] ダウンロードの検証に成功しました。",
        "[3/4] ダウンロードしたディストリビューションをインポート中...", "[4/4] ディストリビューション '{0}' のインストールが完了しました。",
        "{0} スレッドでダウンロード", "このディストリビューションにはダウンロード URL がありません",
        "{0} 回試行後、ダウンロードに失敗: {1}"),
    "ko": (
        'url = "URL 다운로드"', "배포판 소스 URL 선택", "사용자 지정 URL",
        "사용자 지정 URL 입력...", "로컬 파일 찾아보기", "배포판 가져오기", "가져오는 중...",
        "배포판을 가져오지 못했습니다: {0}", "배포판 선택", "다운로드 스레드 (1-8)", "아키텍처",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} 다운로드 중...", "{0} / {1} MB ({2}%)",
        "SHA256 검증 실패 ({0}/{1}), 재시도 중...", "[2/4] 다운로드 검증 성공.",
        "[3/4] 다운로드한 배포판 가져오는 중...", "[4/4] 배포판 '{0}' 설치 성공.",
        "{0}개 스레드로 다운로드", "이 배포판에 사용 가능한 다운로드 URL이 없습니다",
        "{0}회 시도 후 다운로드 실패: {1}"),
    "nl": (
        'url = "URL-download"', "Selecteer de bron-URL van de distributie", "Aangepaste URL",
        "Voer aangepaste URL in...", "Blader door lokaal bestand", "Distributies ophalen", "Ophalen...",
        "Ophalen van distributies mislukt: {0}", "Selecteer distributie", "Download-threads (1-8)",
        "Architectuur", "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} downloaden...", "{0} / {1} MB ({2}%)",
        "SHA256-verificatie mislukt ({0}/{1}), opnieuw...", "[2/4] Download succesvol geverifieerd.",
        "[3/4] Gedownloade distributie importeren...", "[4/4] Distributie '{0}' succesvol geïnstalleerd.",
        "Downloaden met {0} threads", "Geen download-URL beschikbaar voor deze distributie",
        "Download mislukt na {0} pogingen: {1}"),
    "no": (
        'url = "URL-nedlasting"', "Velg kilde-URL for distribusjon", "Egendefinert URL",
        "Skriv inn egendefinert URL...", "Bla gjennom lokal fil", "Hent distribusjoner", "Henter...",
        "Henting av distribusjoner mislyktes: {0}", "Velg distribusjon", "Nedlastingstråder (1-8)",
        "Arkitektur", "AMD64 (x86_64)", "ARM64",
        "[1/4] Laster ned {0}...", "{0} / {1} MB ({2}%)",
        "SHA256-bekreftelse mislyktes ({0}/{1}), prøver på nytt...", "[2/4] Nedlasting bekreftet.",
        "[3/4] Importerer nedlastet distribusjon...", "[4/4] Distribusjon '{0}' installert.",
        "Nedlasting med {0} tråder", "Ingen nedlastings-URL tilgjengelig for denne distribusjonen",
        "Nedlasting mislyktes etter {0} forsøk: {1}"),
    "pt": (
        'url = "Download por URL"', "Selecione o URL de origem da distribuição", "URL personalizado",
        "Insira URL personalizado...", "Procurar arquivo local", "Buscar distribuições", "Buscando...",
        "Falha ao buscar distribuições: {0}", "Selecionar distribuição", "Threads de download (1-8)",
        "Arquitetura", "AMD64 (x86_64)", "ARM64",
        "[1/4] Baixando {0}...", "{0} / {1} MB ({2}%)",
        "Falha na verificação SHA256 ({0}/{1}), tentando novamente...", "[2/4] Download verificado com sucesso.",
        "[3/4] Importando distribuição baixada...", "[4/4] Distribuição '{0}' instalada com sucesso.",
        "Baixar usando {0} threads", "Nenhum URL de download disponível para esta distribuição",
        "Falha no download após {0} tentativas: {1}"),
    "ru": (
        'url = "Загрузка по URL"', "Выберите URL источника дистрибутива", "Пользовательский URL",
        "Введите пользовательский URL...", "Обзор локального файла", "Получить дистрибутивы",
        "Получение...", "Не удалось получить дистрибутивы: {0}", "Выберите дистрибутив",
        "Потоков загрузки (1-8)", "Архитектура", "AMD64 (x86_64)", "ARM64",
        "[1/4] Загрузка {0}...", "{0} / {1} МБ ({2}%)",
        "Ошибка проверки SHA256 ({0}/{1}), повтор...", "[2/4] Загрузка успешно проверена.",
        "[3/4] Импорт загруженного дистрибутива...", "[4/4] Дистрибутив '{0}' успешно установлен.",
        "Загрузка с использованием {0} потоков", "Нет доступного URL для загрузки этого дистрибутива",
        "Загрузка не удалась после {0} попыток: {1}"),
    "sk": (
        'url = "URL stiahnutie"', "Vyberte URL zdroja distribúcie", "Vlastné URL",
        "Zadajte vlastné URL...", "Prehľadať lokálny súbor", "Získať distribúcie", "Získavanie...",
        "Získavanie distribúcií zlyhalo: {0}", "Vyberte distribúciu", "Sťahovacie vlákna (1-8)",
        "Architektúra", "AMD64 (x86_64)", "ARM64",
        "[1/4] Sťahovanie {0}...", "{0} / {1} MB ({2}%)",
        "SHA256 overenie zlyhalo ({0}/{1}), opakujem...", "[2/4] Stiahnutie overené.",
        "[3/4] Import stiahnutej distribúcie...", "[4/4] Distribúcia '{0}' nainštalovaná.",
        "Sťahovanie pomocou {0} vlákien", "Pre túto distribúciu nie je k dispozícii URL na stiahnutie",
        "Sťahovanie zlyhalo po {0} pokusoch: {1}"),
    "sl": (
        'url = "URL prenos"', "Izberite URL vira distribucije", "URL po meri",
        "Vnesite URL po meri...", "Prebrskaj lokalno datoteko", "Pridobi distribucije", "Pridobivanje...",
        "Pridobivanje distribucij ni uspelo: {0}", "Izberite distribucijo", "Prenosne niti (1-8)",
        "Arhitektura", "AMD64 (x86_64)", "ARM64",
        "[1/4] Prenos {0}...", "{0} / {1} MB ({2}%)",
        "Preverjanje SHA256 ni uspelo ({0}/{1}), ponovno...", "[2/4] Prenos uspešno preverjen.",
        "[3/4] Uvoz prenesene distribucije...", "[4/4] Distribucija '{0}' uspešno nameščena.",
        "Prenos z {0} nitmi", "Ni na voljo URL za prenos te distribucije",
        "Prenos ni uspel po {0} poskusih: {1}"),
    "sv": (
        'url = "URL-nedladdning"', "Välj käll-URL för distribution", "Anpassad URL",
        "Ange anpassad URL...", "Bläddra efter lokal fil", "Hämta distributioner", "Hämtar...",
        "Hämtning av distributioner misslyckades: {0}", "Välj distribution", "Nedladdningstrådar (1-8)",
        "Arkitektur", "AMD64 (x86_64)", "ARM64",
        "[1/4] Laddar ner {0}...", "{0} / {1} MB ({2}%)",
        "SHA256-verifiering misslyckades ({0}/{1}), försöker igen...", "[2/4] Nedladdning verifierad.",
        "[3/4] Importerar nedladdad distribution...", "[4/4] Distribution '{0}' installerad.",
        "Nedladdning med {0} trådar", "Ingen nedladdnings-URL tillgänglig för denna distribution",
        "Nedladdning misslyckades efter {0} försök: {1}"),
    "tr": (
        'url = "URL \u0130ndirme"',
        "Da\u011f\u0131t\u0131m kaynak URL'sini se\u00e7in", "\u00d6zel URL",
        "\u00d6zel URL girin...", "Yerel dosyaya g\u00f6z at", "Da\u011f\u0131t\u0131mlar\u0131 getir",
        "Getiriliyor...", "Da\u011f\u0131t\u0131mlar getirilemedi: {0}", "Da\u011f\u0131t\u0131m se\u00e7",
        "\u0130ndirme i\u015f par\u00e7ac\u0131\u011f\u0131 (1-8)", "Mimari",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} indiriliyor...", "{0} / {1} MB ({2}%)",
        "SHA256 do\u011frulamas\u0131 ba\u015far\u0131s\u0131z ({0}/{1}), yeniden deneniyor...",
        "[2/4] \u0130ndirme ba\u015far\u0131yla do\u011fruland\u0131.",
        "[3/4] \u0130ndirilen da\u011f\u0131t\u0131m i\u00e7e aktar\u0131l\u0131yor...",
        "[4/4] Da\u011f\u0131t\u0131m '{0}' ba\u015far\u0131yla kuruldu.",
        "{0} i\u015f par\u00e7ac\u0131\u011f\u0131 kullanarak indir",
        "Bu da\u011f\u0131t\u0131m i\u00e7in kullan\u0131labilir indirme URL'si yok",
        "{0} denemeden sonra indirme ba\u015far\u0131s\u0131z: {1}"),
    "ur": (
        'url = "URL ڈاؤن لوڈ"', "ڈسٹری بیوشن سورس یو آر ایل منتخب کریں", "کسٹم URL",
        "کسٹم URL درج کریں...", "مقامی فائل براؤز کریں", "ڈسٹری بیوشنز حاصل کریں",
        "حاصل کیا جا رہا ہے...", "ڈسٹری بیوشنز حاصل کرنے میں ناکام: {0}",
        "ڈسٹری بیوشن منتخب کریں", "ڈاؤن لوڈ تھریڈز (1-8)", "آرکیٹیکچر",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] {0} ڈاؤن لوڈ ہو رہا ہے...", "{0} / {1} MB ({2}%)",
        "SHA256 تصدیق ناکام ({0}/{1})، دوبارہ کوشش...", "[2/4] ڈاؤن لوڈ کامیابی سے تصدیق شدہ۔",
        "[3/4] ڈاؤن لوڈ کردہ ڈسٹری بیوشن امپورٹ ہو رہی ہے...", "[4/4] ڈسٹری بیوشن '{0}' کامیابی سے انسٹال ہو گئی۔",
        "{0} تھریڈز استعمال کرتے ہوئے ڈاؤن لوڈ کریں", "اس ڈسٹری بیوشن کے لیے کوئی ڈاؤن لوڈ URL دستیاب نہیں",
        "{0} کوششوں کے بعد ڈاؤن لوڈ ناکام: {1}"),
    "en": (
        'url = "URL Download"', "Select distribution source URL", "Custom URL",
        "Enter custom URL...", "Browse Local File", "Fetch Distributions", "Fetching...",
        "Failed to fetch distributions: {0}", "Select distribution",
        "Download threads (1-8)", "Architecture", "AMD64 (x86_64)", "ARM64",
        "[1/4] Downloading {0}...", "{0} / {1} MB ({2}%)",
        "SHA256 verification failed ({0}/{1}), retrying...", "[2/4] Download verified successfully.",
        "[3/4] Importing downloaded distribution...", "[4/4] Distribution '{0}' installed successfully.",
        "Download using {0} threads", "No download URL available for this distribution",
        "Download failed after {0} attempts: {1}"),
    "zh-CN": (
        'url = "URL 下载"', "选择发行版来源 URL", "自定义 URL",
        "请输入自定义 URL...", "浏览本地文件", "获取发行版列表", "正在获取...",
        "获取发行版列表失败：{0}", "选择发行版", "下载线程数（1-8）", "架构选择",
        "AMD64 (x86_64)", "ARM64",
        "[1/4] 正在下载 {0}...", "{0} / {1} MB（{2}%）",
        "SHA256 验证失败（{0}/{1}），正在重试...", "[2/4] 下载验证成功。",
        "[3/4] 正在导入下载的发行版...", "[4/4] 发行版 '{0}' 安装成功。",
        "使用 {0} 个线程下载", "该发行版没有可用的下载地址",
        "下载失败，已重试 {0} 次：{1}"),
}

def build_sections(d):
    (url_str, srcUrl, custom, customPH, browse, fetchBtn, fetchProg,
     fetchFailed, distro_name, threads, arch, x64, arm64,
     downloading, mb, shaFail, shaOk, importing, installed,
     usingThreads, noUrl, dlFailed) = d

    lines = []
    lines.append("[add.url]")
    lines.append(f'select_source_url = "{srcUrl}"')
    lines.append(f'gitee = "Gitee Mirror"')
    lines.append(f'github = "GitHub"')
    lines.append(f'custom = "{custom}"')
    lines.append(f'custom_url = "{custom}"')
    lines.append(f'custom_placeholder = "{customPH}"')
    lines.append(f'browse = "{browse}"')
    lines.append(f'fetch = "{fetchBtn}"')
    lines.append(f'fetching = "{fetchProg}"')
    lines.append(f'fetch_failed = "{fetchFailed}"')
    lines.append(f'select_distro = "{distro_name}"')
    lines.append(f'download_threads = "{threads}"')
    lines.append(f'select_arch = "{arch}"')
    lines.append(f'x64 = "{x64}"')
    lines.append(f'arm64 = "{arm64}"')
    lines.append("")
    lines.append("[install.url]")
    lines.append(f'step_1_4 = "{downloading}"')
    lines.append(f'step_1_4_progress = "{mb}"')
    lines.append(f'step_2_4_failed = "{shaFail}"')
    lines.append(f'step_2_4_done = "{shaOk}"')
    lines.append(f'step_3_4 = "{importing}"')
    lines.append(f'step_4_4 = "{installed}"')
    lines.append(f'step_threads = "{usingThreads}"')
    lines.append(f'step_no_url = "{noUrl}"')
    lines.append(f'step_download_failed = "{dlFailed}"')
    return "\n".join(lines)

def process_file(filepath, lang):
    d = DATA[lang]
    url_str = d[0]

    with open(filepath, 'r', encoding='utf-8-sig') as f:
        content = f.read()

    # 1. Insert json = "JSON" after vhdx = "VHDX"
    content = re.sub(
        r'^(vhdx = "VHDX")',
        r'\1\njson = "JSON"',
        content,
        flags=re.MULTILINE
    )

    # 2. Insert url = "..." after store = "..." (just the line, no trailing ws)
    content = re.sub(
        r'^(store = "[^"]*")',
        r'\1\n' + url_str,
        content,
        flags=re.MULTILINE
    )

    # 3. Build and insert [add.url] and [install.url] before [distro]
    #    Use '\\1' to get regex backreference (not Python octal escape)
    sections = build_sections(d)
    content = re.sub(
        r'^(\[distro\])',
        sections + '\n\n\\1',
        content,
        flags=re.MULTILINE
    )

    with open(filepath, 'w', encoding='utf-8-sig') as f:
        f.write(content)
    print(f"  OK - {lang}")

def main():
    print("Starting translation completion (fixed version)...")
    SKIP = set()
    for lang in sorted(DATA.keys()):
        fname = f"{lang}.toml"
        filepath = os.path.join(BASE, fname)
        if not os.path.exists(filepath):
            print(f"  SKIP - {fname} not found")
            continue
        if lang in SKIP:
            print(f"  SKIP - {fname}")
            continue
        process_file(filepath, lang)
    print("All done!")

if __name__ == "__main__":
    main()
