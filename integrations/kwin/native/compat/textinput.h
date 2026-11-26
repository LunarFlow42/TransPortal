// SPDX-FileCopyrightText: 2020 Bhushan Shah <bshah@kde.org>
// SPDX-License-Identifier: LGPL-2.1-only OR LGPL-3.0-only OR LicenseRef-KDE-Accepted-LGPL
// KWin 6.3.6 public text-input enums. Some SDKs ship textinput_v3.h
// without this sibling header. The SDK's own header wins when present.
#pragma once
#include <QMetaType>
#include <QtGlobal>

namespace KWin {
enum class TextInputContentHint {
    None = 0, AutoCompletion = 1 << 0, AutoCorrection = 1 << 1,
    AutoCapitalization = 1 << 2, LowerCase = 1 << 3, UpperCase = 1 << 4,
    TitleCase = 1 << 5, HiddenText = 1 << 6, SensitiveData = 1 << 7,
    Latin = 1 << 8, MultiLine = 1 << 9,
};
Q_DECLARE_FLAGS(TextInputContentHints, TextInputContentHint)
enum class TextInputContentPurpose {
    Normal, Alpha, Digits, Number, Phone, Url, Email, Name, Password,
    Date, Time, DateTime, Terminal, Pin,
};
enum class TextInputChangeCause { InputMethod, Other };
}
Q_DECLARE_METATYPE(KWin::TextInputContentHint)
Q_DECLARE_METATYPE(KWin::TextInputContentHints)
Q_DECLARE_OPERATORS_FOR_FLAGS(KWin::TextInputContentHints)
Q_DECLARE_METATYPE(KWin::TextInputContentPurpose)
Q_DECLARE_METATYPE(KWin::TextInputChangeCause)
