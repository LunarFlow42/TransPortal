#include "gesture.h"

#include <core/inputdevice.h>
#include <input_event.h>
#include <input_event_spy.h>
#include <plugin.h>
#include <wayland/clientconnection.h>
#include <wayland/seat.h>
#include <wayland/surface.h>
#include <wayland/textinput_v2.h>
#include <wayland/textinput_v3.h>
#include <wayland_server.h>

#include <QDBusConnection>
#include <QDBusConnectionInterface>
#include <QDBusContext>
#include <QDBusMessage>
#include <QDBusServiceWatcher>
#include <QFileInfo>
#include <QSettings>
#include <QTimer>
#ifdef TRANSPORTAL_KWIN_SMOKE_TESTS
#include <QDateTime>
#endif

namespace {
constexpr auto Path = "/org/kde/KWin/TransPortalGestures";
constexpr auto Interface = "org.kde.KWin.TransPortalGestures1";
using namespace Transportal;

Key keyFor(Qt::Key key)
{
    switch (key) {
    case Qt::Key_C: return Key::C;
    case Qt::Key_Control: return Key::Control;
    case Qt::Key_Shift: return Key::Shift;
    case Qt::Key_Alt: return Key::Alt;
    case Qt::Key_CapsLock: return Key::CapsLock;
    case Qt::Key_QuoteLeft: case Qt::Key_AsciiTilde: return Key::Backquote;
    default:
        if (key >= Qt::Key_F1 && key <= Qt::Key_F12) return Key(int(Key::F1) + key - Qt::Key_F1);
        return Key::Other;
    }
}

std::optional<Key> inkKey(int number)
{
    switch (number) {
    case 1: return Key::Backquote;
    case 2: return Key::CapsLock;
    case 3: return Key::Control;
    case 4: return Key::Alt;
    case 5: return Key::Shift;
    default:
        if (number >= 11 && number <= 22) return Key(int(Key::F1) + number - 11);
        return {};
    }
}

class GesturePlugin final : public KWin::Plugin, public KWin::InputEventSpy, protected QDBusContext
{
    Q_OBJECT
    Q_CLASSINFO("D-Bus Interface", "org.kde.KWin.TransPortalGestures1")

public:
    GesturePlugin()
        : m_bus(QDBusConnection::sessionBus())
    {
        m_watch.setConnection(m_bus);
        m_watch.setWatchMode(QDBusServiceWatcher::WatchForUnregistration);
        connect(&m_watch, &QDBusServiceWatcher::serviceUnregistered, this, [this](const QString &name) {
            if (name == m_owner) clearOwner();
        });
        connect(KWin::waylandServer()->seat(), &KWin::SeatInterface::focusedKeyboardSurfaceAboutToChange,
                this, [this] { ++m_epoch; m_detector.reset(); cancelReplace(); m_inputTicket = 0; updateSpy(); });
        m_expiry.setSingleShot(true);
        connect(&m_expiry, &QTimer::timeout, this, [this] { cancelReplace(); });
        auto *seat = KWin::waylandServer()->seat();
        watchTextInput(seat->textInputV2());
        watchTextInput(seat->textInputV3());
        if (auto *v3 = seat->textInputV3())
            connect(v3, &KWin::TextInputV3Interface::enableRequested, this, [this] { cancelReplace(); });
        m_registered = m_bus.registerObject(QString::fromLatin1(Path), this,
                                            QDBusConnection::ExportScriptableSlots);
    }

    ~GesturePlugin() override
    {
        // InputEventSpy's destructor unregisters even after plugin unload.
        if (m_registered) m_bus.unregisterObject(QString::fromLatin1(Path));
    }

public Q_SLOTS:
    Q_SCRIPTABLE bool Configure(bool doubleCopy, int ink, bool suspended)
    {
        if (!calledFromDBus() || (!m_owner.isEmpty() && m_owner != message().service())
            || ink < 0 || (ink != 0 && !inkKey(ink))) return false;
        if (!authorizedClient(false)) {
            if (m_owner == message().service()) clearOwner();
            return false;
        }
        if (m_owner.isEmpty()) {
            m_owner = message().service();
            m_watch.addWatchedService(m_owner);
        }
        const auto key = inkKey(ink);
        if (m_copy != doubleCopy || m_ink != key || m_suspended != suspended) {
            m_detector.reset();
            ++m_epoch;
        }
        m_copy = doubleCopy;
        m_ink = key;
        m_suspended = suspended;
        if (suspended) { cancelReplace(); m_inputTicket = 0; }
        updateSpy();
        return true;
    }

    Q_SCRIPTABLE bool MatchesFocus(uint process, qulonglong epoch)
    {
        return calledFromDBus() && m_owner == message().service() && matchesFocus(process, epoch);
    }

    Q_SCRIPTABLE bool ReplacementAvailable()
    {
        return authorizedEditor();
    }

    Q_SCRIPTABLE bool InputAvailable()
    {
        return calledFromDBus() && !m_owner.isEmpty() && message().service() == m_owner
            && authorizedClient(false) && m_remoteInputGranted;
    }

    Q_SCRIPTABLE bool RemoteInputSupported()
    {
        return calledFromDBus() && !m_owner.isEmpty() && message().service() == m_owner
            && authorizedClient(false);
    }

    Q_SCRIPTABLE bool SetRemoteInput(bool enabled)
    {
        if (!calledFromDBus() || m_owner.isEmpty() || message().service() != m_owner
            || !authorizedClient(false)) return false;
        m_remoteInputGranted = enabled;
        if (!enabled) m_inputTicket = 0;
        updateSpy();
        return true;
    }

    Q_SCRIPTABLE qulonglong ArmInput(uint process)
    {
        if (!InputAvailable() || m_suspended || process == 0 || process == ownerProcess()
            || focusedProcess() != process) return 0;
        m_inputTicket = m_epoch;
        updateSpy();
        return m_inputTicket;
    }

    Q_SCRIPTABLE bool ValidateInput(uint process, qulonglong ticket)
    {
        return InputAvailable() && !m_suspended && ticket != 0 && ticket == m_inputTicket
            && ticket == m_epoch && process != 0 && focusedProcess() == process;
    }

    Q_SCRIPTABLE void CancelInput(qulonglong ticket)
    {
        if (calledFromDBus() && message().service() == m_owner && ticket == m_inputTicket) {
            m_inputTicket = 0;
            updateSpy();
        }
    }

#ifdef TRANSPORTAL_KWIN_SMOKE_TESTS
    // Exists only in a separately compiled disposable smoke-test plugin. This
    // is not installed by default and cannot receive test events in release builds.
    Q_SCRIPTABLE bool TestKeyEvent(int key, uint scan, int state, bool ctrlOnly, bool noOtherModifiers)
    {
        if (!calledFromDBus() || m_owner.isEmpty() || message().service() != m_owner
            || !authorizedClient(false) || focusedProcess() == 0 || !m_running) return false;
        const Key mapped = keyFor(Qt::Key(key));
        if (mapped != Key::C && mapped != Key::Control && mapped != Key::Shift && mapped != Key::Other) return false;
        if (state < int(Edge::Press) || state > int(Edge::Repeat)) return false;
        KWin::KeyboardKeyEvent event{};
        event.key = Qt::Key(key);
        event.nativeScanCode = scan;
        event.state = state == int(Edge::Press) ? KWin::KeyboardKeyState::Pressed
            : state == int(Edge::Release) ? KWin::KeyboardKeyState::Released : KWin::KeyboardKeyState::Repeated;
        event.modifiersRelevantForGlobalShortcuts = ctrlOnly ? Qt::ControlModifier
            : noOtherModifiers ? Qt::NoModifier : Qt::AltModifier;
        event.timestamp = std::chrono::milliseconds(QDateTime::currentMSecsSinceEpoch());
        m_testEvent = true;
        keyboardKey(&event);
        m_testEvent = false;
        return true;
    }

    Q_SCRIPTABLE bool TestConfigure(bool copy, int ink, bool suspended)
    {
        return Configure(copy, ink, suspended);
    }
#endif

    Q_SCRIPTABLE qulonglong ArmReplacement(uint process, const QString &expected)
    {
        if (!authorizedEditor() || m_suspended || focusedProcess() != process || process == 0) return 0;
        auto *seat = KWin::waylandServer()->seat();
        const bool v2 = matchesSelection(seat->textInputV2(), expected);
        const bool v3 = matchesSelection(seat->textInputV3(), expected);
        if (v2 == v3) return 0; // refuse absent or ambiguous targets
        // Single use, valid only while the same surface and selection survive.
        m_ticket = ++m_epoch;
        m_useV3 = v3;
        m_expiry.start(30000);
        updateSpy();
        return m_ticket;
    }

    Q_SCRIPTABLE bool CommitReplacement(uint process, qulonglong ticket, const QString &expected,
                                         const QString &replacement)
    {
        if (!authorizedEditor() || ticket == 0 || ticket != m_ticket) return false;
        cancelReplace();
        if (m_suspended || focusedProcess() != process || process == 0 || replacement.trimmed().isEmpty()
            || replacement.toUtf8().size() > 400000 || replacement.contains(QChar(u'\0'))) return false;
        auto *seat = KWin::waylandServer()->seat();
        // The text-input protocol commits over the current selection. No
        // global input keys, clipboard ownership, or Delete/Insert pair.
        if (m_useV3) {
            auto *text = seat->textInputV3();
            if (!matchesSelection(text, expected)) return false;
            text->commitString(replacement);
            text->done();
        } else {
            auto *text = seat->textInputV2();
            if (!matchesSelection(text, expected)) return false;
            text->commitString(replacement);
        }
        return true;
    }

    Q_SCRIPTABLE void CancelReplacement(qulonglong ticket)
    {
        if (calledFromDBus() && message().service() == m_owner && ticket == m_ticket) cancelReplace();
    }

private:
    bool authorizedEditor() const
    {
        return calledFromDBus() && !m_owner.isEmpty() && message().service() == m_owner && authorizedClient(true);
    }

    bool authorizedClient(bool editor) const
    {
        if (!calledFromDBus()) return false;
        // Explicit local opt-in. A session-bus name alone is not authority to
        // edit text: bind to a user-configured, canonical executable via DBus PID.
        QSettings config(QStringLiteral("TransPortal"), QStringLiteral("KWinIntegration"));
        if (editor && !config.value(QStringLiteral("EnableInPlaceReplacement"), false).toBool()) return false;
        const auto configured = config.value(QStringLiteral("ApplicationExecutable")).toString();
        if (!configured.startsWith(u'/') || QFileInfo(configured).canonicalFilePath() != configured) return false;
        const auto pid = m_bus.interface()->servicePid(message().service());
        return pid.isValid() && pid.value() > 0 &&
            QFileInfo(QStringLiteral("/proc/%1/exe").arg(pid.value())).canonicalFilePath() == configured;
    }

    uint ownerProcess() const
    {
        if (m_owner.isEmpty()) return 0;
        const auto pid = m_bus.interface()->servicePid(m_owner);
        return pid.isValid() && pid.value() > 0 ? uint(pid.value()) : 0;
    }

    template<class Input>
    void watchTextInput(Input *text)
    {
        if (!text) return;
        connect(text, &Input::enabledChanged, this, [this] { cancelReplace(); });
        connect(text, &Input::surroundingTextChanged, this, [this] { cancelReplace(); });
        connect(text, &Input::contentTypeChanged, this, [this] { cancelReplace(); });
    }

    template<class Input>
    bool matchesSelection(Input *text, const QString &expected) const
    {
        const auto expectedBytes = expected.toUtf8();
        if (expected.trimmed().isEmpty() || expectedBytes.size() > 4000) return false;
        auto *seat = KWin::waylandServer()->seat();
        if (!text || !text->isEnabled() || text->surface() != seat->focusedKeyboardSurface()
            || text->contentPurpose() != KWin::TextInputContentPurpose::Normal
            || text->contentHints().testAnyFlags(KWin::TextInputContentHint::HiddenText | KWin::TextInputContentHint::SensitiveData)) return false;
        const auto start = std::min(text->surroundingTextCursorPosition(), text->surroundingTextSelectionAnchor());
        const auto end = std::max(text->surroundingTextCursorPosition(), text->surroundingTextSelectionAnchor());
        // The compositor already holds this protocol buffer. Compare only the
        // selected bytes in place; never send surrounding text out of KWin.
        const auto buffer = text->surroundingText().toUtf8();
        return start >= 0 && end > start && end <= buffer.size()
            && QByteArrayView(buffer).sliced(start, end - start) == expectedBytes;
    }

    void updateSpy()
    {
        const bool running = !m_owner.isEmpty() && !m_suspended && (m_ticket || m_inputTicket || m_copy || m_ink.has_value());
        if (running == m_running) return;
        if (running) KWin::input()->installInputEventSpy(this);
        else KWin::input()->uninstallInputEventSpy(this);
        m_running = running;
    }

    void cancelReplace()
    {
        m_ticket = 0;
        m_inputTicket = 0;
        m_expiry.stop();
        updateSpy();
    }

    uint focusedProcess() const
    {
        auto *server = KWin::waylandServer();
        if (server->isScreenLocked()) return 0;
        auto *surface = server->seat()->focusedKeyboardSurface();
        if (!surface || surface->client() == server->xWaylandConnection()) return 0;
        const auto pid = surface->client()->processId();
        return pid > 0 ? uint(pid) : 0;
    }

    bool matchesFocus(uint process, qulonglong epoch) const
    {
        return m_running && process != 0 && m_epoch == epoch && focusedProcess() == process;
    }

    void clearOwner()
    {
        m_watch.removeWatchedService(m_owner);
        m_owner.clear();
        m_remoteInputGranted = false;
        cancelReplace();
        m_detector.reset();
        ++m_epoch;
    }

    void pointerButton(KWin::PointerButtonEvent *) override
    {
        invalidateInput();
    }

    void invalidateInput()
    {
        m_detector.reset();
        ++m_epoch;
        if (m_ticket) QTimer::singleShot(0, this, [this] { updateSpy(); });
        m_ticket = 0;
    }

    void keyboardKey(KWin::KeyboardKeyEvent *event) override
    {
        if (!m_running || m_owner.isEmpty()) return;
        bool native = event->device &&
            QByteArrayView(event->device->metaObject()->className()) == "KWin::LibInput::Device";
#ifdef TRANSPORTAL_KWIN_SMOKE_TESTS
        native = native || m_testEvent;
#endif
        if (m_inputTicket && native && event->state != KWin::KeyboardKeyState::Released) {
            ++m_epoch;
            m_inputTicket = 0;
            QTimer::singleShot(0, this, [this] { updateSpy(); });
        }
        if (m_ticket && native && event->state != KWin::KeyboardKeyState::Released) {
            m_ticket = 0;
            // Never mutate the spy list while KWin is iterating through it.
            QTimer::singleShot(0, this, [this] { updateSpy(); });
        }
        const uint pid = focusedProcess();
        // Native libinput devices only: no EIS, virtual keyboard, fake-input,
        // or internal repeat-source events can start a gesture.
        if (!native) return; // RemoteDesktop/EIS input is not a passive gesture.
        if (!pid) {
            m_detector.reset();
            ++m_epoch;
            return;
        }
        const auto key = keyFor(event->key);
        auto mods = event->modifiersRelevantForGlobalShortcuts;
        const bool ctrlOnly = mods == Qt::ControlModifier;
        if (key == Key::Control) mods &= ~Qt::ControlModifier;
        if (key == Key::Alt) mods &= ~Qt::AltModifier;
        if (key == Key::Shift) mods &= ~Qt::ShiftModifier;
        const auto edge = event->state == KWin::KeyboardKeyState::Pressed ? Edge::Press
            : event->state == KWin::KeyboardKeyState::Released ? Edge::Release : Edge::Repeat;
        const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(event->timestamp).count();
        processKey(key, event->nativeScanCode, edge, ctrlOnly, mods == Qt::NoModifier, pid, ms);
    }

    void processKey(Key key, quint32 scan, Edge edge, bool ctrlOnly, bool noOtherModifiers, uint pid, qint64 ms)
    {
        if (edge != Edge::Release) ++m_epoch;
        const auto gesture = m_detector.key(key, scan, edge,
                                           ctrlOnly, noOtherModifiers, ms, m_copy, m_ink);
        if (!gesture) return;
        const auto epoch = m_epoch;
        const auto owner = m_owner;
        const auto kind = *gesture == Gesture::DoubleCopy ? QStringLiteral("copy") : QStringLiteral("ink");
        // Queue outside the keyboard stack; the original event always reaches
        // KWin's regular filters and is never accepted or reinjected here.
        QTimer::singleShot(0, this, [this, pid, epoch, owner, kind] {
            if (owner != m_owner || !matchesFocus(pid, epoch)) return;
            auto signal = QDBusMessage::createTargetedSignal(owner, QString::fromLatin1(Path),
                QString::fromLatin1(Interface), QStringLiteral("Triggered"));
            signal << kind << pid << epoch;
            m_bus.send(signal);
        });
    }

    QDBusConnection m_bus;
    QDBusServiceWatcher m_watch;
    QString m_owner;
    Detector m_detector;
    qulonglong m_epoch = 1;
    qulonglong m_ticket = 0;
    qulonglong m_inputTicket = 0;
    bool m_useV3 = false;
    QTimer m_expiry;
    std::optional<Key> m_ink;
    bool m_registered = false, m_running = false, m_copy = false, m_suspended = false, m_remoteInputGranted = false;
#ifdef TRANSPORTAL_KWIN_SMOKE_TESTS
    bool m_testEvent = false;
#endif
};

class GestureFactory final : public KWin::PluginFactory
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID PluginFactory_iid FILE "metadata.json")
    Q_INTERFACES(KWin::PluginFactory)

public:
    std::unique_ptr<KWin::Plugin> create() const override
    {
        if (!KWin::waylandServer() || !KWin::waylandServer()->seat() || !KWin::input()) return {};
        return std::make_unique<GesturePlugin>();
    }
};
}

#include "plugin.moc"
