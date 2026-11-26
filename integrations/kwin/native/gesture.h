#pragma once
#include <cstdint>
#include <optional>

namespace Transportal {

enum class Key { Other, C, Control, Shift, Alt, CapsLock, Backquote, F1 };
enum class Edge { Press, Release, Repeat };
enum class Gesture { DoubleCopy, Ink };

// Only a small set of logical keys enter this classifier. No text or raw key
// sequences are retained. Times are milliseconds from a monotonic clock.
class Detector {
public:
    std::optional<Gesture> key(Key key, unsigned scan, Edge edge, bool ctrlOnly,
                              bool noOtherModifiers, std::int64_t now, bool copyEnabled,
                              std::optional<Key> ink)
    {
        if (edge == Edge::Repeat) {
            reset();
            return {};
        }
        if (edge == Edge::Press) {
            if (ink && key == *ink && noOtherModifiers && !m_inkHeld && !m_copyHeld) {
                m_inkHeld = scan;
                m_inkStart = now;
            } else {
                m_inkHeld.reset();
            }
            if (copyEnabled && key == Key::C && ctrlOnly && !m_copyHeld) {
                m_copyHeld = scan;
                m_copyStart = now;
            } else if (key != Key::Control) {
                m_copyHeld.reset();
                m_firstCopy.reset();
            }
            return {};
        }
        if (m_copyHeld == scan) {
            m_copyHeld.reset();
            m_inkHeld.reset();
            if (!copyEnabled || now < m_copyStart || now - m_copyStart > 350) {
                m_firstCopy.reset();
                return {};
            }
            if (m_firstCopy && now >= *m_firstCopy && now - *m_firstCopy <= 450) {
                m_firstCopy.reset();
                return Gesture::DoubleCopy;
            }
            m_firstCopy = now;
        } else if (m_inkHeld == scan) {
            m_inkHeld.reset();
            if (ink && now >= m_inkStart && now - m_inkStart <= 350) {
                m_firstCopy.reset();
                return Gesture::Ink;
            }
        } else if (key != Key::Control) {
            reset();
        }
        return {};
    }

    void reset() { *this = Detector{}; }

private:
    std::optional<unsigned> m_copyHeld, m_inkHeld;
    std::optional<std::int64_t> m_firstCopy;
    std::int64_t m_copyStart = 0, m_inkStart = 0;
};

}
