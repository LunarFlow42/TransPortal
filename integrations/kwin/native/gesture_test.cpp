#include "gesture.h"
#include <cstdlib>
#include <iostream>

using namespace Transportal;

static void require(bool value, const char *message)
{
    if (!value) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}

int main()
{
    Detector detector;
    auto key = [&](Key k, unsigned scan, Edge edge, int time, bool ctrl = false, bool alone = true) {
        return detector.key(k, scan, edge, ctrl, alone, time, true, Key::Shift);
    };
    auto copy = [&](int time) {
        require(!key(Key::C, 46, Edge::Press, time, true), "press must not emit");
        return key(Key::C, 46, Edge::Release, time + 20, true);
    };
    require(!copy(10), "first copy");
    key(Key::Control, 29, Edge::Release, 50);
    key(Key::Control, 29, Edge::Press, 100);
    require(copy(130) == Gesture::DoubleCopy, "double copy with separate Control presses");
    require(!copy(200), "no triple copy");
    require(!copy(700), "old copy expires");
    key(Key::Other, 1, Edge::Press, 750);
    require(!copy(800), "other key cancels sequence");
    key(Key::C, 46, Edge::Press, 900, true);
    key(Key::C, 46, Edge::Repeat, 940, true);
    require(!key(Key::C, 46, Edge::Release, 960, true), "repeat isn't a tap");
    require(!copy(1000), "repeat clears previous copy");
    detector.reset();
    require(!copy(1100), "focus change resets sequence");
    key(Key::Shift, 42, Edge::Press, 1200);
    require(key(Key::Shift, 42, Edge::Release, 1240) == Gesture::Ink, "ink tap");
    key(Key::Shift, 42, Edge::Press, 1300);
    key(Key::Other, 30, Edge::Press, 1320);
    require(!key(Key::Shift, 42, Edge::Release, 1360), "typing with modifier isn't ink");
    key(Key::Shift, 42, Edge::Press, 1400);
    require(!key(Key::Shift, 42, Edge::Release, 1850), "held ink key expires");
    key(Key::Shift, 42, Edge::Press, 1900, false, false);
    require(!key(Key::Shift, 42, Edge::Release, 1920), "modified ink key isn't ink");
    require(!detector.key(Key::C, 46, Edge::Press, true, true, 1930, false, {}), "disabled copy");
    require(!detector.key(Key::C, 46, Edge::Release, true, true, 1940, false, {}), "disabled copy release");
    std::cout << "Gesture detector: independent taps, cancellation, repeat, timeout and opt-out passed\n";
}
