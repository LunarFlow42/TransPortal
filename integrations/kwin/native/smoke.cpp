// Real Qt editor in a disposable KWin session. No provider calls or access
// to the developer's desktop. Qt's v3 client advertises both selection offsets.
#include <QApplication>
#include <QClipboard>
#include <QDBusConnection>
#include <QDBusInterface>
#include <QDBusReply>
#include <QLineEdit>
#include <QProcess>
#include <QTimer>

template<class T, class... Args>
static T call(QDBusInterface &iface, const char *method, Args... args)
{
    QDBusReply<T> reply = iface.call(QString::fromLatin1(method), args...);
    if (!reply.isValid()) qFatal("%s: %s", method, qPrintable(reply.error().message()));
    return reply.value();
}

int main(int argc, char **argv)
{
    QApplication app(argc, argv);
    QApplication::setDesktopFileName(QStringLiteral("com.transportal.app"));
    QDBusInterface iface(QStringLiteral("org.kde.KWin"), QStringLiteral("/org/kde/KWin/TransPortalGestures"),
                         QStringLiteral("org.kde.KWin.TransPortalGestures1"));
    iface.setTimeout(1000);
    // Same user and bus, but a different executable must not claim ownership.
    QProcess untrusted;
    untrusted.start(QStringLiteral("gdbus"), {QStringLiteral("call"), QStringLiteral("--session"),
        QStringLiteral("--dest"), QStringLiteral("org.kde.KWin"), QStringLiteral("--object-path"),
        QStringLiteral("/org/kde/KWin/TransPortalGestures"), QStringLiteral("--method"),
        QStringLiteral("org.kde.KWin.TransPortalGestures1.Configure"), QStringLiteral("true"),
        QStringLiteral("5"), QStringLiteral("false")});
    if (!untrusted.waitForFinished(1500) || untrusted.exitCode() != 0 || untrusted.readAllStandardOutput().trimmed() != "(false,)")
        qFatal("Untrusted executable was not rejected");
    const bool rustReplace = qEnvironmentVariableIsSet("RUST_REPLACEMENT_PROBE");
    const bool rustGestures = qEnvironmentVariableIsSet("RUST_GESTURE_PROBE");
    if (!rustReplace && !rustGestures) {
        if (!call<bool>(iface, "Configure", true, 5, false)) qFatal("Plugin attachment failed");
        if (!call<bool>(iface, "ReplacementAvailable")) qFatal("Fixture executable was not authorized");
    }
    QLineEdit entry(QStringLiteral("before original after"));
    QLineEdit overlay(QStringLiteral("Read-only placement fixture"));
    overlay.setWindowTitle(QStringLiteral("TransPortal Overlay [1]"));
    entry.setWindowTitle(QStringLiteral("TransPortal test editor"));
    entry.show();
    entry.setFocus();
    auto pid = uint(QCoreApplication::applicationPid());
    const QString original = QStringLiteral("original");
    const QString replacement = QStringLiteral("译文🙂");
    const QString sentinel = QStringLiteral("unchanged clipboard");
    int attempts = 0;
    bool probed = false, invalidationTested = false;
    qulonglong invalidated = 0;
    auto verify = [&] {
        if (entry.text() != QStringLiteral("before 译文🙂 after"))
            qFatal("Qt entry was not replaced in place: %s", qPrintable(entry.text()));
        if (QApplication::clipboard()->text() != sentinel) qFatal("Clipboard was altered");
        qInfo("Real KWin/Qt: selected range replaced, clipboard and surrounding text unchanged");
        if (!rustReplace) qInfo("Real KWin/Qt: untrusted caller, changed selection and ticket replay rejected");
        overlay.show();
        QTimer::singleShot(400, &entry, [&] {
            if (!entry.isActiveWindow() || overlay.isActiveWindow()) qFatal("Overlay stole focus");
            qInfo("Real KWin/Qt: marked overlay mapped without taking keyboard focus");
            app.quit();
        });
    };
    QProcess probe;
    QObject::connect(&probe, &QProcess::finished, &entry, [&](int code, QProcess::ExitStatus status) {
        if (status != QProcess::NormalExit || code != 0) qFatal("Real AT-SPI selection probe failed");
        if (rustReplace) QTimer::singleShot(250, &entry, verify);
        if (rustGestures) {
            if (entry.text() != QStringLiteral("before original after") || entry.selectedText() != original
                || QApplication::clipboard()->text() != sentinel) qFatal("Passive gesture modified editor or clipboard");
            qInfo("Qt fixture: synthetic copy/ink events did not edit text or clipboard");
            overlay.show();
            QTimer::singleShot(400, &entry, [&] {
                if (!entry.isActiveWindow() || overlay.isActiveWindow()) qFatal("Overlay stole focus");
                qInfo("Real KWin/Qt: marked overlay mapped without taking keyboard focus");
                app.quit();
            });
        }
    });
    QTimer timer;
    QObject::connect(&timer, &QTimer::timeout, &entry, [&] {
        if (++attempts > 30) qFatal("No focused text-input-v3 selection");
        if (!entry.isActiveWindow()) return;
        if (attempts < 4 || entry.selectedText() != original) {
            entry.setSelection(7, 8);
            if (QApplication::clipboard()->text() != sentinel) QApplication::clipboard()->setText(sentinel);
            return;
        }
        if (rustReplace || rustGestures) {
            timer.stop();
            probe.setProcessChannelMode(QProcess::ForwardedChannels);
            probe.start(qEnvironmentVariable(rustGestures ? "RUST_GESTURE_PROBE" : "RUST_REPLACEMENT_PROBE"),
                         {rustGestures ? QStringLiteral("platform::linux::gestures::tests::real_kwin_copy_and_ink_events_reach_rust_and_read_the_selected_range")
                                       : QStringLiteral("platform::linux::selection::tests::real_kwin_replacement"),
                         QStringLiteral("--exact"), QStringLiteral("--ignored"), QStringLiteral("--nocapture")});
            if (!probe.waitForStarted(1000)) qFatal("Could not start Rust replacement probe");
            return;
        }
        if (!probed && qEnvironmentVariableIsSet("RUST_SELECTION_PROBE")) {
            probed = true;
            probe.setProcessChannelMode(QProcess::ForwardedChannels);
            probe.start(qEnvironmentVariable("RUST_SELECTION_PROBE"),
                        {QStringLiteral("platform::linux::atspi::tests::real_qt_selection"),
                         QStringLiteral("--exact"), QStringLiteral("--ignored"), QStringLiteral("--nocapture")});
            if (!probe.waitForStarted(1000)) qFatal("Could not start the Rust verification binary");
            return;
        }
        if (probe.state() != QProcess::NotRunning) return;
        if (invalidated) {
            if (call<bool>(iface, "CommitReplacement", pid, invalidated, original, replacement))
                qFatal("A changed-and-restored selection accepted the old ticket");
            invalidated = 0;
        }
        const auto arm = [&] { return call<qulonglong>(iface, "ArmReplacement", pid, original); };
        auto ticket = arm();
        if (!ticket) return;
        if (!invalidationTested) {
            invalidationTested = true;
            invalidated = ticket;
            entry.setSelection(0, 6);
            return;
        }
        timer.stop();
        const auto commit = [&](const QString &expected, const QString &text) {
            return call<bool>(iface, "CommitReplacement", pid, ticket, expected, text);
        };
        if (commit(QStringLiteral("wrong"), replacement) || commit(original, replacement))
            qFatal("Mismatched selection or reused ticket accepted");
        if (!(ticket = arm()) || !commit(original, replacement) || commit(original, replacement))
            qFatal("Valid selection rejected or successful ticket replayed");
        QTimer::singleShot(250, &entry, verify);
    });
    timer.start(150);
    return app.exec();
}
