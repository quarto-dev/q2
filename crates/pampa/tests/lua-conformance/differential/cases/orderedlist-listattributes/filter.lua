-- Class C1: OrderedList's second argument (ListAttributes).
-- q2 currently discards it, hardcoding (1, Default, Default).
function Para(p)
  return pandoc.OrderedList({ { pandoc.Plain(p.content) } },
                            { 3, 'Decimal', 'Period' })
end
