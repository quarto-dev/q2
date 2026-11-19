import EmbedDemo from '../EmbedDemo';
import { mount } from '../mount';
// After EmbedDemo, so these rules follow hub-client's stylesheets and win
// at equal specificity. The theme is the only difference from embed.html.
import '../LavaTheme.css';

mount(<EmbedDemo />);
